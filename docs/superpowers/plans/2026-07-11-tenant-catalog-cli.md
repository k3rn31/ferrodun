# Tenant Catalogue + `mudd` Subcommand CLI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove `tenant_tag` from tenant config, introduce an operator-side tenant catalogue that assigns ports and tags, and restructure `mudd` into a subcommand CLI (`serve`, `tenant add/remove/list`).

**Architecture:** The 12-bit tenant tag is runtime-only (EntityIds are re-minted at boot), so it moves from builder config to a machine-owned `catalog.toml` sibling of the server config, which also assigns each tenant's port (lowest free ≥ `base_port`, reuse after removal). `mudd` becomes a clap subcommand CLI; `serve` builds the boot registry from the catalogue; `tenant add` scaffolds a minimal bootable world.

**Tech Stack:** Rust, clap (derive, subcommands), figment (TOML + `MUDD_` env), serde + `toml` crate (catalogue), tokio, jj for VCS.

**Spec:** `docs/superpowers/specs/2026-07-11-tenant-catalog-cli-design.md`

## Global Constraints

- `unwrap()` is strictly forbidden everywhere; `expect()` only in tests, with a descriptive message. Integration-test files under `tests/` need `#![allow(clippy::expect_used, clippy::panic)]` with the standard comment (see `crates/mudd/tests/telnet_login.rs:4`).
- `mudd` is an application: errors via `anyhow` with `.context()`. No `panic!`/`todo!`/`unreachable!`.
- Newtype pattern: domain concepts get types (`TenantName`, `TenantTag`); raw primitives must not cross public APIs where a domain meaning exists (exception: `u16` ports, the std convention).
- Never `println!`/`eprintln!` (workspace denies `print_stdout`/`print_stderr`): CLI output goes through `writeln!` to a locked stdout handle, propagating errors.
- Doc comments on all public APIs (behavior, params, errors — not implementation).
- Never suppress lints without a `// LINT:` comment in the smallest scope.
- Add dependencies with `cargo add`, never by hand-editing Cargo.toml.
- VCS is **jj**, not git: commit with `jj commit -m "message" <files>` (never `git`).
- TDD: failing test → minimal code → refactor. Run the named test before and after implementing.
- Unit tests live in `#[cfg(test)] mod tests` at the bottom of the file they cover.
- Logging discipline: `tenant` subcommands are plain CLI paths — no tracing setup, no new log events. Only `serve` installs the subscriber.
- The catalogue is machine-owned: no env or flag overrides for its contents.

---

### Task 1: `TenantEntry` carries the tag; boot consumes it

The tag stops flowing through `TenantConfig` (builder config) and instead
rides the operator-side `TenantEntry`. Interim: `[[tenants]]` file entries
get positional tags (1, 2, …) until Task 9 replaces the array with the
catalogue. The duplicate-tag boot check is deleted (the catalogue will
guarantee uniqueness by construction).

**Files:**
- Modify: `crates/mudd/src/config.rs` (TenantEntry, RawServerConfig, resolve, tests)
- Modify: `crates/mudd/src/boot.rs`
- Test: `crates/mudd/tests/telnet_login.rs`

**Interfaces:**
- Consumes: `mud_core::TenantTag` (`new(u16) -> Result`, `get() -> u16`, `Default` = 0, `Copy`).
- Produces: `pub struct TenantEntry { pub dir: PathBuf, pub listen: SocketAddr, pub tag: TenantTag }` — no longer `Serialize`/`Deserialize`. Every later task that builds a boot registry uses this shape.

- [ ] **Step 1: Update the integration tests to the new shapes (they must fail to compile)**

In `crates/mudd/tests/telnet_login.rs`:

1. Add `use mud_core::TenantTag;` to the imports.
2. `write_tenant` loses its `tag` parameter and no longer writes `tenant_tag`:

```rust
/// Writes a minimal, self-contained tenant directory: one region, one room
/// with no exits, and a welcome banner (no dangling references).
fn write_tenant(dir: &Path) {
    std::fs::write(dir.join("config.toml"), "start_room = \"town_square\"\n")
        .expect("write config.toml");
    std::fs::write(
        dir.join("welcome.kdl"),
        "banner \"Welcome to Testville.\"\n",
    )
    .expect("write welcome.kdl");

    let world = dir.join("world/town");
    std::fs::create_dir_all(&world).expect("create world dir");
    std::fs::write(
        world.join("region.kdl"),
        "region \"town\" {\n    name \"Town\"\n}\n",
    )
    .expect("write region.kdl");
    std::fs::write(
        world.join("town.kdl"),
        "room \"town_square\" {\n    title \"Town Square\"\n    description \"A test square.\"\n}\n",
    )
    .expect("write town.kdl");
}
```

3. Every `write_tenant(dir.path(), N)` call site becomes `write_tenant(dir.path())`.
4. Every `TenantEntry` literal gains a `tag`. In `single_tenant_config`:

```rust
        tenants: vec![TenantEntry {
            dir: dir.to_path_buf(),
            listen: "127.0.0.1:0".parse().expect("addr"),
            tag: TenantTag::new(1).expect("tag 1 is in range"),
        }],
```

In `two_tenants_serve_independent_logins_at_once`, tenant a gets
`tag: TenantTag::new(1).expect("tag 1 is in range")` and tenant b gets
`tag: TenantTag::new(2).expect("tag 2 is in range")`.

5. Delete the whole `duplicate_tenant_tags_fail_boot` test — tag uniqueness
   becomes the catalogue's invariant (tested in Task 5), not boot's.

- [ ] **Step 2: Verify the compile failure**

Run: `cargo test -p mudd --test telnet_login`
Expected: FAIL — `TenantEntry` has no field `tag`.

- [ ] **Step 3: Add the field and rewire boot**

In `crates/mudd/src/config.rs`:

1. Add `use mud_core::TenantTag;` to the imports.
2. Split the authored shape from the runtime shape:

```rust
/// One registered tenant: its folder, telnet listen address, and the runtime
/// tenant tag stamped into its `EntityId`s.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantEntry {
    pub dir: PathBuf,
    pub listen: SocketAddr,
    pub tag: TenantTag,
}

/// The `[[tenants]]` shape as authored in the server config file.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RawTenantEntry {
    dir: PathBuf,
    listen: SocketAddr,
}
```

3. `RawServerConfig.tenants` becomes `Vec<RawTenantEntry>`.
4. In `resolve()`, map raw entries to positional tags (interim until Task 9):

```rust
        let tenants = match &cli.tenant_dir {
            Some(dir) => vec![TenantEntry {
                dir: dir.clone(),
                listen: cli.listen.unwrap_or(DEFAULT_LISTEN),
                tag: TenantTag::default(),
            }],
            None => raw
                .tenants
                .into_iter()
                .enumerate()
                .map(|(index, raw)| {
                    let position = u16::try_from(index + 1)
                        .ok()
                        .and_then(|value| TenantTag::new(value).ok())
                        .ok_or_else(|| {
                            anyhow::anyhow!("too many tenants: at most 4095 fit in one process")
                        })?;
                    Ok(TenantEntry {
                        dir: raw.dir,
                        listen: raw.listen,
                        tag: position,
                    })
                })
                .collect::<anyhow::Result<Vec<_>>>()?,
        };
```

5. Fix the existing config tests: `defaults_apply_when_the_config_file_is_absent`
   expects `tag: TenantTag::default()` in its `TenantEntry`; add
   `use mud_core::TenantTag;` inside `mod tests`.

In `crates/mudd/src/boot.rs`:

1. Remove `use std::collections::HashSet;`.
2. `PersistentWorld::load(db.clone(), tenant_config.tenant_tag(), place_map)` becomes `PersistentWorld::load(db.clone(), entry.tag, place_map)`.
3. The tenant span becomes `tenant = entry.tag.get()`.
4. Delete the `tenant_tags` vector, the `tenant_tags.push(...)` line, and the whole duplicate-tag `HashSet` loop at the end; keep `addrs.push(bound_addr);`.
5. Update the doc comment on `boot`: drop the "or if two tenants share a `tenant_tag`" clause from `# Errors`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p mudd`
Expected: PASS (config unit tests + both remaining telnet integration tests).

- [ ] **Step 5: Commit**

```bash
jj commit -m "refactor(mudd): TenantEntry carries the runtime tenant tag" crates/mudd
```

---

### Task 2: Strip `tenant_tag` from `TenantConfig` (mud-world)

Tenant `config.toml` becomes purely builder content, as PLAN M1-12
specified. Nothing consumes `TenantConfig::tenant_tag()` after Task 1.

**Files:**
- Modify: `crates/mud-world/src/config.rs`
- Modify: `crates/mud-world/src/error.rs`

**Interfaces:**
- Produces: `TenantConfig` without `tenant_tag`/`tenant_tag()`; `WorldError` without `TenantTagOutOfRange`. No caller may reference either from here on.

- [ ] **Step 1: Make the removal fail first — delete the field's tests and add a guard test**

In `crates/mud-world/src/config.rs` tests:

1. Delete `an_out_of_range_tenant_tag_is_rejected`.
2. Replace `tenant_tag_and_locale_are_exposed` and
   `tenant_tag_and_locale_default_when_absent` with locale-only versions,
   plus a test pinning the new contract — an authored `tenant_tag` is
   simply ignored (figment tolerates unknown keys), not an error:

```rust
    #[test]
    fn locale_is_exposed() {
        figment::Jail::expect_with(|jail| {
            jail.create_file(
                "config.toml",
                "start_room = \"town_square\"\nlocale = \"fr\"",
            )?;
            let config = TenantConfig::load(jail.directory()).expect("config loads");

            assert_eq!(config.locale().as_str(), "fr");
            Ok(())
        });
    }

    #[test]
    fn locale_defaults_when_absent() {
        figment::Jail::expect_with(|jail| {
            jail.create_file("config.toml", "start_room = \"town_square\"")?;
            let config = TenantConfig::load(jail.directory()).expect("config loads");

            assert_eq!(config.locale().as_str(), "en");
            Ok(())
        });
    }

    #[test]
    fn a_stale_tenant_tag_key_is_ignored() {
        figment::Jail::expect_with(|jail| {
            jail.create_file(
                "config.toml",
                "start_room = \"town_square\"\ntenant_tag = 3",
            )?;
            let config = TenantConfig::load(jail.directory()).expect("config loads");

            assert_eq!(config.start_room(), "town_square");
            Ok(())
        });
    }
```

- [ ] **Step 2: Run the new tests (they pass — this is a removal task, so the "red" is the workspace compile check after Step 3)**

Run: `cargo test -p mud-world config`
Expected: PASS for the three new tests (the field still exists; figment ignores the stale key either way).

- [ ] **Step 3: Remove the field, accessor, validation, and error variant**

In `crates/mud-world/src/config.rs`:

1. Delete the `tenant_tag: u16` field (and its doc comment) from `TenantConfig`.
2. Delete the `tenant_tag()` accessor.
3. Delete the `let _: TenantTag = TenantTag::try_from(...)` validation in `load()`.
4. Remove `use mud_core::TenantTag;`.
5. In `load()`'s doc comment, drop the `WorldError::TenantTagOutOfRange` sentence.

In `crates/mud-world/src/error.rs`: delete the `TenantTagOutOfRange(u16)` variant and its doc comment.

- [ ] **Step 4: Verify the whole workspace still compiles and passes**

Run: `cargo test --workspace`
Expected: PASS. If anything still references `tenant_tag()` or `TenantTagOutOfRange`, the compiler lists it — those references were all removed in Task 1; fix any stragglers by taking the tag from `TenantEntry` instead.

- [ ] **Step 5: Commit**

```bash
jj commit -m "refactor(mud-world): tenant config is builder content only, drop tenant_tag" crates/mud-world
```

---

### Task 3: `TenantName` newtype

**Files:**
- Create: `crates/mudd/src/catalog.rs`
- Modify: `crates/mudd/src/lib.rs`

**Interfaces:**
- Produces: `catalog::TenantName` with `parse(&str) -> anyhow::Result<TenantName>`, `as_str(&self) -> &str`, `Display`, `Clone`, `PartialEq`, `Eq`, serde via `try_from = "String"` / `into = "String"`. Later tasks use `TenantName` for every tenant-name parameter — never a raw `&str` past the CLI boundary.

- [ ] **Step 1: Create the module skeleton and write the failing tests**

Create `crates/mudd/src/catalog.rs`:

```rust
//! The tenant catalogue: the operator-side registry that assigns each
//! tenant its listen port and runtime tenant tag (design:
//! docs/superpowers/specs/2026-07-11-tenant-catalog-cli-design.md).

use std::fmt;

use serde::{Deserialize, Serialize};

/// A tenant's name: lowercase ASCII alphanumeric plus `-`/`_`, starting with
/// an alphanumeric. It doubles as the tenant's folder name under
/// `tenants_dir`, so the grammar is deliberately filesystem-safe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TenantName(String);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_simple_slug_parses() {
        let name = TenantName::parse("my-game_2").expect("valid slug");
        assert_eq!(name.as_str(), "my-game_2");
    }

    #[test]
    fn uppercase_is_rejected() {
        assert!(TenantName::parse("MyGame").is_err());
    }

    #[test]
    fn a_leading_separator_is_rejected() {
        assert!(TenantName::parse("-game").is_err());
        assert!(TenantName::parse("_game").is_err());
    }

    #[test]
    fn empty_and_path_like_names_are_rejected() {
        assert!(TenantName::parse("").is_err());
        assert!(TenantName::parse("a/b").is_err());
        assert!(TenantName::parse("..").is_err());
    }
}
```

Register the module in `crates/mudd/src/lib.rs`:

```rust
pub mod catalog;
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p mudd catalog`
Expected: FAIL — `parse` and `as_str` are not defined.

- [ ] **Step 3: Implement `TenantName`**

Add below the struct in `crates/mudd/src/catalog.rs`:

```rust
impl TenantName {
    /// Parses a tenant name, rejecting anything but lowercase ASCII
    /// alphanumerics, `-`, and `_` (the first character must be
    /// alphanumeric).
    ///
    /// # Errors
    ///
    /// Returns an error naming the offending value when the grammar is
    /// violated.
    pub fn parse(value: &str) -> anyhow::Result<TenantName> {
        let mut chars = value.chars();
        let starts_alnum = chars
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
        let rest_valid = chars
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
        if !(starts_alnum && rest_valid) {
            anyhow::bail!(
                "invalid tenant name {value:?}: use lowercase letters, digits, `-`, `_`, starting with a letter or digit"
            );
        }
        Ok(TenantName(value.to_owned()))
    }

    /// The name as authored.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TenantName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for TenantName {
    type Error = anyhow::Error;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        TenantName::parse(&value)
    }
}

impl From<TenantName> for String {
    fn from(name: TenantName) -> String {
        name.0
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p mudd catalog`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
jj commit -m "feat(mudd): TenantName slug newtype for the tenant catalogue" crates/mudd
```

---

### Task 4: Catalogue model, load, validate, save

**Files:**
- Modify: `crates/mudd/src/catalog.rs`
- Modify: `crates/mudd/Cargo.toml` (via `cargo add toml -p mudd`)

**Interfaces:**
- Consumes: `TenantName` (Task 3), `mud_core::TenantTag`.
- Produces:
  - `pub struct CatalogEntry { pub name: TenantName, pub port: u16, pub tag: TenantTag }` (`Clone`, `PartialEq`, `Eq`)
  - `pub struct Catalog` (`Default`) with `load(path: &Path) -> anyhow::Result<Catalog>`, `save(&self, path: &Path) -> anyhow::Result<()>`, `entries(&self) -> &[CatalogEntry]`

- [ ] **Step 1: Add the `toml` dependency**

Run: `cargo add toml -p mudd`

- [ ] **Step 2: Write the failing tests**

Append to `mod tests` in `crates/mudd/src/catalog.rs` (add `use std::path::Path;` etc. as needed; `tempfile` is already a dev-dependency):

```rust
    use mud_core::TenantTag;

    fn tag(value: u16) -> TenantTag {
        TenantTag::new(value).expect("test tag is in range")
    }

    #[test]
    fn a_missing_file_is_an_empty_catalog() {
        let dir = tempfile::tempdir().expect("temp dir");
        let catalog = Catalog::load(&dir.path().join("catalog.toml")).expect("loads");
        assert!(catalog.entries().is_empty());
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("catalog.toml");
        let catalog = Catalog {
            entries: vec![CatalogEntry {
                name: TenantName::parse("alpha").expect("slug"),
                port: 4000,
                tag: tag(1),
            }],
        };
        catalog.save(&path).expect("saves");

        let reloaded = Catalog::load(&path).expect("loads");
        assert_eq!(reloaded, catalog);
    }

    #[test]
    fn duplicate_ports_in_the_file_are_rejected() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("catalog.toml");
        std::fs::write(
            &path,
            "[[tenants]]\nname = \"a\"\nport = 4000\ntag = 1\n\n[[tenants]]\nname = \"b\"\nport = 4000\ntag = 2\n",
        )
        .expect("write");
        assert!(Catalog::load(&path).is_err(), "duplicate port must be rejected");
    }

    #[test]
    fn duplicate_names_and_tags_are_rejected() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("catalog.toml");
        std::fs::write(
            &path,
            "[[tenants]]\nname = \"a\"\nport = 4000\ntag = 1\n\n[[tenants]]\nname = \"a\"\nport = 4001\ntag = 2\n",
        )
        .expect("write");
        assert!(Catalog::load(&path).is_err(), "duplicate name must be rejected");

        std::fs::write(
            &path,
            "[[tenants]]\nname = \"a\"\nport = 4000\ntag = 1\n\n[[tenants]]\nname = \"b\"\nport = 4001\ntag = 1\n",
        )
        .expect("write");
        assert!(Catalog::load(&path).is_err(), "duplicate tag must be rejected");
    }

    #[test]
    fn out_of_range_tags_are_rejected() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("catalog.toml");
        std::fs::write(&path, "[[tenants]]\nname = \"a\"\nport = 4000\ntag = 0\n")
            .expect("write");
        assert!(Catalog::load(&path).is_err(), "tag 0 is reserved for dev mode");

        std::fs::write(&path, "[[tenants]]\nname = \"a\"\nport = 4000\ntag = 5000\n")
            .expect("write");
        assert!(Catalog::load(&path).is_err(), "tag above 4095 must be rejected");
    }
```

(The round-trip test builds `Catalog` by struct literal: `entries` is
private but visible here because `mod tests` is a child module.)

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p mudd catalog`
Expected: FAIL — `Catalog`, `CatalogEntry` not defined.

- [ ] **Step 4: Implement the model**

Add to `crates/mudd/src/catalog.rs` (new imports at the top: `use std::collections::HashSet; use std::path::Path; use anyhow::Context; use mud_core::TenantTag;`):

```rust
/// One catalogue row: a tenant and its assigned runtime values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    pub name: TenantName,
    pub port: u16,
    pub tag: TenantTag,
}

/// The tenant catalogue, as loaded from `catalog.toml`.
///
/// The file is machine-owned — `mudd tenant` subcommands are its only
/// writers — but hand-edits are validated on load: unique names, ports, and
/// tags, with tags in `1..=TenantTag::MAX` (tag 0 is reserved for
/// `--tenant-dir` dev mode).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Catalog {
    entries: Vec<CatalogEntry>,
}

/// The on-disk shape of `catalog.toml`.
#[derive(Debug, Default, Serialize, Deserialize)]
struct RawCatalog {
    #[serde(default)]
    tenants: Vec<RawCatalogEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct RawCatalogEntry {
    name: TenantName,
    port: u16,
    tag: u16,
}

impl Catalog {
    /// Loads the catalogue from `path`. A missing file is an empty
    /// catalogue.
    ///
    /// # Errors
    ///
    /// Returns an error if the file is unreadable or malformed, or if it
    /// violates a catalogue invariant (duplicate name/port/tag, or a tag
    /// outside `1..=TenantTag::MAX`).
    pub fn load(path: &Path) -> anyhow::Result<Catalog> {
        if !path.exists() {
            return Ok(Catalog::default());
        }
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading tenant catalogue {}", path.display()))?;
        let raw: RawCatalog = toml::from_str(&text)
            .with_context(|| format!("parsing tenant catalogue {}", path.display()))?;

        let mut entries = Vec::with_capacity(raw.tenants.len());
        let mut names = HashSet::new();
        let mut ports = HashSet::new();
        let mut tags = HashSet::new();
        for raw_entry in raw.tenants {
            let tag = (raw_entry.tag >= 1)
                .then(|| TenantTag::new(raw_entry.tag).ok())
                .flatten()
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "{}: tenant {:?} has tag {} outside 1..={}",
                        path.display(),
                        raw_entry.name.as_str(),
                        raw_entry.tag,
                        TenantTag::MAX,
                    )
                })?;
            if !names.insert(raw_entry.name.clone()) {
                anyhow::bail!(
                    "{}: duplicate tenant name {:?}",
                    path.display(),
                    raw_entry.name.as_str()
                );
            }
            if !ports.insert(raw_entry.port) {
                anyhow::bail!("{}: duplicate port {}", path.display(), raw_entry.port);
            }
            if !tags.insert(tag) {
                anyhow::bail!("{}: duplicate tag {}", path.display(), tag.get());
            }
            entries.push(CatalogEntry {
                name: raw_entry.name,
                port: raw_entry.port,
                tag,
            });
        }
        Ok(Catalog { entries })
    }

    /// Serializes the whole catalogue to `path`, replacing the file.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization or the write fails.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let raw = RawCatalog {
            tenants: self
                .entries
                .iter()
                .map(|entry| RawCatalogEntry {
                    name: entry.name.clone(),
                    port: entry.port,
                    tag: entry.tag.get(),
                })
                .collect(),
        };
        let text = toml::to_string_pretty(&raw).context("serializing tenant catalogue")?;
        std::fs::write(path, text)
            .with_context(|| format!("writing tenant catalogue {}", path.display()))?;
        Ok(())
    }

    /// The registered tenants, in registration order.
    #[must_use]
    pub fn entries(&self) -> &[CatalogEntry] {
        &self.entries
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p mudd catalog`
Expected: PASS (all catalog tests, including Task 3's).

- [ ] **Step 6: Commit**

```bash
jj commit -m "feat(mudd): tenant catalogue model with validated load/save" crates/mudd
```

---

### Task 5: Catalogue assignment — `add` and `remove`

**Files:**
- Modify: `crates/mudd/src/catalog.rs`

**Interfaces:**
- Produces:
  - `Catalog::add(&mut self, name: TenantName, base_port: u16) -> anyhow::Result<CatalogEntry>` — assigns lowest free port ≥ `base_port` and lowest free tag ≥ 1; freed values are reused; duplicate name is an error.
  - `Catalog::remove(&mut self, name: &TenantName) -> anyhow::Result<CatalogEntry>` — unknown name is an error; returns the removed entry.

- [ ] **Step 1: Write the failing tests**

Append to `mod tests`:

```rust
    fn name(value: &str) -> TenantName {
        TenantName::parse(value).expect("valid test slug")
    }

    #[test]
    fn add_assigns_sequential_ports_and_tags() {
        let mut catalog = Catalog::default();
        let a = catalog.add(name("a"), 4000).expect("add a");
        let b = catalog.add(name("b"), 4000).expect("add b");

        assert_eq!((a.port, a.tag), (4000, tag(1)));
        assert_eq!((b.port, b.tag), (4001, tag(2)));
    }

    #[test]
    fn removed_values_are_reused() {
        let mut catalog = Catalog::default();
        catalog.add(name("a"), 4000).expect("add a");
        catalog.add(name("b"), 4000).expect("add b");
        catalog.remove(&name("a")).expect("remove a");

        let c = catalog.add(name("c"), 4000).expect("add c");
        assert_eq!((c.port, c.tag), (4000, tag(1)), "freed port and tag are reused");
    }

    #[test]
    fn a_duplicate_name_is_rejected() {
        let mut catalog = Catalog::default();
        catalog.add(name("a"), 4000).expect("add a");
        assert!(catalog.add(name("a"), 4000).is_err());
    }

    #[test]
    fn removing_an_unknown_name_is_an_error() {
        let mut catalog = Catalog::default();
        assert!(catalog.remove(&name("ghost")).is_err());
    }

    #[test]
    fn remove_returns_the_entry() {
        let mut catalog = Catalog::default();
        catalog.add(name("a"), 4000).expect("add a");
        let removed = catalog.remove(&name("a")).expect("remove a");
        assert_eq!((removed.port, removed.tag), (4000, tag(1)));
        assert!(catalog.entries().is_empty());
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p mudd catalog`
Expected: FAIL — `add`/`remove` not defined.

- [ ] **Step 3: Implement assignment**

Add to `impl Catalog`:

```rust
    /// Registers `name`, assigning the lowest free port `>= base_port` and
    /// the lowest free tag `>= 1`. Values freed by [`remove`](Catalog::remove)
    /// are reused.
    ///
    /// # Errors
    ///
    /// Returns an error if the name is already registered, or if the port or
    /// tag space is exhausted.
    pub fn add(&mut self, name: TenantName, base_port: u16) -> anyhow::Result<CatalogEntry> {
        if self.entries.iter().any(|entry| entry.name == name) {
            anyhow::bail!("tenant {name} is already registered");
        }
        let used_ports: HashSet<u16> = self.entries.iter().map(|entry| entry.port).collect();
        let port = (base_port..=u16::MAX)
            .find(|candidate| !used_ports.contains(candidate))
            .ok_or_else(|| anyhow::anyhow!("no free port at or above {base_port}"))?;
        let used_tags: HashSet<TenantTag> = self.entries.iter().map(|entry| entry.tag).collect();
        let tag = (1..=TenantTag::MAX)
            .filter_map(|candidate| TenantTag::new(candidate).ok())
            .find(|candidate| !used_tags.contains(candidate))
            .ok_or_else(|| anyhow::anyhow!("all {} tenant tags are in use", TenantTag::MAX))?;

        let entry = CatalogEntry { name, port, tag };
        self.entries.push(entry.clone());
        Ok(entry)
    }

    /// Deregisters `name`, freeing its port and tag for reuse, and returns
    /// the removed entry.
    ///
    /// # Errors
    ///
    /// Returns an error if no tenant with that name is registered.
    pub fn remove(&mut self, name: &TenantName) -> anyhow::Result<CatalogEntry> {
        let index = self
            .entries
            .iter()
            .position(|entry| &entry.name == name)
            .ok_or_else(|| anyhow::anyhow!("no tenant named {name} is registered"))?;
        Ok(self.entries.remove(index))
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p mudd catalog`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
jj commit -m "feat(mudd): catalogue port and tag assignment with reuse" crates/mudd
```

---

### Task 6: Tenant folder scaffold

**Files:**
- Create: `crates/mudd/src/scaffold.rs`
- Modify: `crates/mudd/src/lib.rs`

**Interfaces:**
- Consumes: `catalog::TenantName`; `mud_world::{TenantConfig, load_world}` (test only).
- Produces:
  - `pub enum Scaffolded { Created, Registered }`
  - `pub fn ensure_tenant_dir(dir: &Path, name: &TenantName) -> anyhow::Result<Scaffolded>` — creates a minimal bootable world if `dir` is absent; returns `Registered` untouched if `dir` exists with a `config.toml`; errors if `dir` exists without one. Never overwrites existing files.

- [ ] **Step 1: Create the module and write the failing tests**

Create `crates/mudd/src/scaffold.rs`:

```rust
//! Scaffolds a new tenant folder with a minimal bootable world, so
//! `mudd serve` works the moment `mudd tenant add` finishes.

use std::path::Path;

use anyhow::Context;

use crate::catalog::TenantName;

/// What [`ensure_tenant_dir`] found or did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scaffolded {
    /// The folder was created and populated with the starter world.
    Created,
    /// The folder already existed with a `config.toml`; it was registered
    /// as-is and left untouched.
    Registered,
}

#[cfg(test)]
mod tests {
    use mud_world::{TenantConfig, load_world};

    use super::*;

    fn name(value: &str) -> TenantName {
        TenantName::parse(value).expect("valid test slug")
    }

    #[test]
    fn a_fresh_scaffold_is_bootable() {
        let root = tempfile::tempdir().expect("temp dir");
        let dir = root.path().join("mygame");

        let outcome = ensure_tenant_dir(&dir, &name("mygame")).expect("scaffold");
        assert_eq!(outcome, Scaffolded::Created);

        let config = TenantConfig::load(&dir).expect("scaffolded config loads");
        assert_eq!(config.start_room(), "start");
        load_world(&config).expect("scaffolded world loads");
    }

    #[test]
    fn an_existing_tenant_dir_is_registered_untouched() {
        let root = tempfile::tempdir().expect("temp dir");
        let dir = root.path().join("mygame");
        std::fs::create_dir_all(&dir).expect("create dir");
        std::fs::write(dir.join("config.toml"), "start_room = \"town_square\"\n")
            .expect("write config");

        let outcome = ensure_tenant_dir(&dir, &name("mygame")).expect("register");
        assert_eq!(outcome, Scaffolded::Registered);

        let config = std::fs::read_to_string(dir.join("config.toml")).expect("read back");
        assert_eq!(
            config, "start_room = \"town_square\"\n",
            "existing files must never be overwritten"
        );
        assert!(!dir.join("world").exists(), "no scaffold on an existing dir");
    }

    #[test]
    fn a_dir_without_config_is_an_error() {
        let root = tempfile::tempdir().expect("temp dir");
        let dir = root.path().join("mygame");
        std::fs::create_dir_all(&dir).expect("create dir");

        assert!(ensure_tenant_dir(&dir, &name("mygame")).is_err());
    }
}
```

Register in `crates/mudd/src/lib.rs`:

```rust
pub mod scaffold;
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p mudd scaffold`
Expected: FAIL — `ensure_tenant_dir` not defined.

- [ ] **Step 3: Implement the scaffold**

Add to `crates/mudd/src/scaffold.rs`:

```rust
/// Ensures `dir` holds a tenant: scaffolds a minimal bootable world when the
/// folder does not exist, or registers an existing folder as-is.
///
/// The starter world has one region and one room (`start`), a
/// `config.toml` pointing at it, and a welcome banner — enough for
/// `mudd serve` to boot and a player to connect immediately.
///
/// # Errors
///
/// Returns an error if `dir` exists but holds no `config.toml` (a
/// half-formed folder the operator must resolve), or on any filesystem
/// failure.
pub fn ensure_tenant_dir(dir: &Path, name: &TenantName) -> anyhow::Result<Scaffolded> {
    if dir.exists() {
        if dir.join("config.toml").exists() {
            return Ok(Scaffolded::Registered);
        }
        anyhow::bail!(
            "{} exists but has no config.toml; remove it or author a tenant config there",
            dir.display()
        );
    }

    let region_dir = dir.join("world").join("start");
    std::fs::create_dir_all(&region_dir)
        .with_context(|| format!("creating tenant world dir {}", region_dir.display()))?;
    write_new(&dir.join("config.toml"), "start_room = \"start\"\n")?;
    write_new(
        &dir.join("welcome.kdl"),
        &format!("banner \"Welcome to {name}.\"\n"),
    )?;
    write_new(
        &region_dir.join("region.kdl"),
        "region \"start\" {\n    name \"Start\"\n}\n",
    )?;
    write_new(
        &region_dir.join("start.kdl"),
        "room \"start\" {\n    title \"Start\"\n    description \"A quiet starting room. Edit world/start/start.kdl to begin building.\"\n}\n",
    )?;
    Ok(Scaffolded::Created)
}

/// Writes a scaffold file, reporting the path on failure.
fn write_new(path: &Path, contents: &str) -> anyhow::Result<()> {
    std::fs::write(path, contents)
        .with_context(|| format!("writing scaffold file {}", path.display()))
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p mudd scaffold`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
jj commit -m "feat(mudd): minimal bootable tenant scaffold" crates/mudd
```

---

### Task 7: `Settings` — server config keys for the catalogue era

Adds the figment-resolved `Settings` (with `tenants_dir`, `bind`,
`base_port`, `catalog_path`) and the catalogue→registry mapping. The old
flat `Cli`/`ServerConfig::resolve` stays alive until Task 9 cuts over.

**Files:**
- Modify: `crates/mudd/src/config.rs`
- Modify: `crates/mudd/src/lib.rs`

**Interfaces:**
- Consumes: `catalog::{Catalog, CatalogEntry}` (Tasks 4–5), `TenantEntry` (Task 1).
- Produces:
  - `pub struct Settings { pub rate: SustainedRate, pub burst: Burst, pub log_format: LogFormat, pub tenants_dir: PathBuf, pub bind: IpAddr, pub base_port: u16, pub catalog_path: PathBuf }`
  - `pub struct Overrides { pub rate: Option<NonZeroU32>, pub burst: Option<NonZeroU32>, pub log_format: Option<LogFormat> }` (`Default`)
  - `Settings::resolve(config: Option<&Path>, overrides: &Overrides) -> anyhow::Result<Settings>`
  - `pub fn tenants_from_catalog(settings: &Settings, catalog: &Catalog) -> anyhow::Result<Vec<TenantEntry>>`

- [ ] **Step 1: Write the failing tests**

Append to `mod tests` in `crates/mudd/src/config.rs`:

```rust
    use std::net::IpAddr;

    use crate::catalog::{Catalog, TenantName};

    #[test]
    fn settings_defaults_apply_when_the_config_file_is_absent() {
        figment::Jail::expect_with(|jail| {
            jail.set_env("XDG_CONFIG_HOME", jail.directory().display());
            jail.set_env("XDG_DATA_HOME", jail.directory().display());
            let settings =
                Settings::resolve(None, &Overrides::default()).expect("settings resolve");

            assert_eq!(settings.rate, SustainedRate::DEFAULT);
            assert_eq!(settings.burst, Burst::DEFAULT);
            assert_eq!(settings.log_format, LogFormat::Text);
            assert_eq!(
                settings.tenants_dir,
                jail.directory().join("ferrodun").join("tenants")
            );
            assert_eq!(settings.bind, "127.0.0.1".parse::<IpAddr>().expect("ip"));
            assert_eq!(settings.base_port, 4000);
            assert_eq!(
                settings.catalog_path,
                jail.directory().join("ferrodun").join("catalog.toml")
            );
            Ok(())
        });
    }

    #[test]
    fn settings_read_the_file_and_env_overrides_it() {
        figment::Jail::expect_with(|jail| {
            jail.create_file(
                "config.toml",
                r#"
rate = 5
bind = "0.0.0.0"
base_port = 5000
tenants_dir = "/srv/ferrodun/tenants"
log_format = "json"
"#,
            )?;
            jail.set_env("MUDD_BASE_PORT", "6000");
            let config_path = jail.directory().join("config.toml");
            let settings =
                Settings::resolve(Some(&config_path), &Overrides::default())
                    .expect("settings resolve");

            assert_eq!(settings.bind, "0.0.0.0".parse::<IpAddr>().expect("ip"));
            assert_eq!(settings.base_port, 6000, "env overrides the file");
            assert_eq!(settings.tenants_dir, PathBuf::from("/srv/ferrodun/tenants"));
            assert_eq!(settings.log_format, LogFormat::Json);
            assert_eq!(
                settings.catalog_path,
                jail.directory().join("catalog.toml"),
                "the catalogue sits beside the config file"
            );
            Ok(())
        });
    }

    #[test]
    fn overrides_beat_file_and_env() {
        figment::Jail::expect_with(|jail| {
            jail.create_file("config.toml", "rate = 5")?;
            jail.set_env("MUDD_RATE", "7");
            let config_path = jail.directory().join("config.toml");
            let overrides = Overrides {
                rate: Some(NonZeroU32::new(9).expect("nonzero")),
                ..Overrides::default()
            };
            let settings =
                Settings::resolve(Some(&config_path), &overrides).expect("settings resolve");

            assert_eq!(
                settings.rate,
                SustainedRate::new(NonZeroU32::new(9).expect("nonzero"))
            );
            Ok(())
        });
    }

    #[test]
    fn the_catalog_maps_to_tenant_entries() {
        figment::Jail::expect_with(|jail| {
            jail.set_env("XDG_CONFIG_HOME", jail.directory().display());
            jail.set_env("XDG_DATA_HOME", jail.directory().display());
            let settings =
                Settings::resolve(None, &Overrides::default()).expect("settings resolve");

            let mut catalog = Catalog::default();
            catalog
                .add(TenantName::parse("alpha").expect("slug"), settings.base_port)
                .expect("add alpha");
            let tenants = tenants_from_catalog(&settings, &catalog).expect("mapping");

            assert_eq!(tenants.len(), 1);
            let tenant = tenants.first().expect("one tenant");
            assert_eq!(tenant.dir, settings.tenants_dir.join("alpha"));
            assert_eq!(
                tenant.listen,
                "127.0.0.1:4000".parse().expect("socket addr")
            );
            assert_eq!(tenant.tag.get(), 1);
            Ok(())
        });
    }

    #[test]
    fn an_empty_catalog_is_a_serve_error() {
        figment::Jail::expect_with(|jail| {
            jail.set_env("XDG_CONFIG_HOME", jail.directory().display());
            jail.set_env("XDG_DATA_HOME", jail.directory().display());
            let settings =
                Settings::resolve(None, &Overrides::default()).expect("settings resolve");

            let result = tenants_from_catalog(&settings, &Catalog::default());
            assert!(result.is_err(), "expected an error, got {result:?}");
            Ok(())
        });
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p mudd config`
Expected: FAIL — `Settings`, `Overrides`, `tenants_from_catalog` not defined.

- [ ] **Step 3: Implement `Settings`**

In `crates/mudd/src/config.rs` (new imports: `use std::net::IpAddr;`, `use crate::catalog::Catalog;`):

```rust
/// Server-wide settings resolved from defaults, the config file, `MUDD_`
/// env vars, and flag-level [`Overrides`], in that precedence order.
#[derive(Debug, PartialEq, Eq)]
pub struct Settings {
    pub rate: SustainedRate,
    pub burst: Burst,
    pub log_format: LogFormat,
    /// Root directory holding one folder per tenant.
    pub tenants_dir: PathBuf,
    /// Host address every tenant listener binds to.
    pub bind: IpAddr,
    /// Lowest port the catalogue may assign.
    pub base_port: u16,
    /// The tenant catalogue, always a sibling of the resolved config file.
    pub catalog_path: PathBuf,
}

/// Flag-level overrides, the strongest configuration layer. Decoupled from
/// clap so `Settings` stays testable without argument parsing.
#[derive(Debug, Default)]
pub struct Overrides {
    pub rate: Option<NonZeroU32>,
    pub burst: Option<NonZeroU32>,
    pub log_format: Option<LogFormat>,
}

/// Untyped shape extracted from figment before conversion to [`Settings`].
#[derive(Debug, Serialize, Deserialize)]
struct RawSettings {
    #[serde(default = "default_rate")]
    rate: NonZeroU32,
    #[serde(default = "default_burst")]
    burst: NonZeroU32,
    #[serde(default)]
    log_format: LogFormat,
    tenants_dir: Option<PathBuf>,
    #[serde(default = "default_bind")]
    bind: IpAddr,
    #[serde(default = "default_base_port")]
    base_port: u16,
}

impl Default for RawSettings {
    fn default() -> Self {
        RawSettings {
            rate: default_rate(),
            burst: default_burst(),
            log_format: LogFormat::default(),
            tenants_dir: None,
            bind: default_bind(),
            base_port: default_base_port(),
        }
    }
}

/// Default listener host: loopback, so a fresh install is never publicly
/// exposed by accident.
fn default_bind() -> IpAddr {
    IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)
}

/// Default lowest assignable tenant port.
fn default_base_port() -> u16 {
    4000
}

impl Settings {
    /// Resolves the server settings (defaults < config.toml < `MUDD_` env <
    /// `overrides`). `config` overrides the XDG config-file location.
    ///
    /// # Errors
    ///
    /// Returns an error if the config file is unreadable or malformed, or if
    /// neither `XDG_CONFIG_HOME`/`XDG_DATA_HOME` nor `HOME` is set when a
    /// default location is needed.
    pub fn resolve(config: Option<&Path>, overrides: &Overrides) -> anyhow::Result<Settings> {
        let config_path = match config {
            Some(path) => path.to_path_buf(),
            None => default_config_path()?,
        };
        let raw: RawSettings = Figment::from(Serialized::defaults(RawSettings::default()))
            .merge(Toml::file(&config_path))
            .merge(Env::prefixed("MUDD_"))
            .extract()?;

        let tenants_dir = match raw.tenants_dir {
            Some(dir) => dir,
            None => default_tenants_dir()?,
        };
        let catalog_path = config_path
            .parent()
            .unwrap_or(Path::new(""))
            .join("catalog.toml");

        Ok(Settings {
            rate: SustainedRate::new(overrides.rate.unwrap_or(raw.rate)),
            burst: Burst::new(overrides.burst.unwrap_or(raw.burst)),
            log_format: overrides.log_format.unwrap_or(raw.log_format),
            tenants_dir,
            bind: raw.bind,
            base_port: raw.base_port,
            catalog_path,
        })
    }
}

/// Builds the boot registry from the catalogue: one [`TenantEntry`] per
/// registered tenant, with `dir` derived from `tenants_dir` + name and the
/// listen address from `bind` + the assigned port.
///
/// # Errors
///
/// Returns an error when the catalogue is empty — there is nothing to serve.
pub fn tenants_from_catalog(
    settings: &Settings,
    catalog: &Catalog,
) -> anyhow::Result<Vec<TenantEntry>> {
    if catalog.entries().is_empty() {
        anyhow::bail!("no tenants registered: run `mudd tenant add <name>`");
    }
    Ok(catalog
        .entries()
        .iter()
        .map(|entry| TenantEntry {
            dir: settings.tenants_dir.join(entry.name.as_str()),
            listen: SocketAddr::new(settings.bind, entry.port),
            tag: entry.tag,
        })
        .collect())
}

/// The XDG-standard tenants root: `$XDG_DATA_HOME/ferrodun/tenants`, falling
/// back to `~/.local/share/ferrodun/tenants`.
fn default_tenants_dir() -> anyhow::Result<PathBuf> {
    let data_home = match std::env::var("XDG_DATA_HOME") {
        Ok(value) => PathBuf::from(value),
        Err(_) => {
            let home = std::env::var("HOME")
                .map_err(|_| anyhow::anyhow!("neither XDG_DATA_HOME nor HOME is set"))?;
            PathBuf::from(home).join(".local").join("share")
        }
    };
    Ok(data_home.join("ferrodun").join("tenants"))
}
```

Also refactor the existing `config_file_path(cli: &Cli)` into
`default_config_path() -> anyhow::Result<PathBuf>` (the XDG lookup without
the `cli.config` short-circuit) and have the old `ServerConfig::resolve`
call `cli.config.clone().map_or_else(default_config_path, Ok)?` so both old
and new paths share it. Add `use std::path::Path;` if missing.

Export the new names from `crates/mudd/src/lib.rs`:

```rust
pub use config::{
    Cli, LogFormat, Overrides, ServerConfig, Settings, TenantEntry, tenants_from_catalog,
};
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p mudd`
Expected: PASS (old resolve tests + new Settings tests).

- [ ] **Step 5: Commit**

```bash
jj commit -m "feat(mudd): Settings with tenants_dir/bind/base_port and catalogue mapping" crates/mudd
```

---

### Task 8: `tenant` command functions (add / remove / list)

Pure functions with injected IO, so the CLI wiring in Task 9 is a thin
dispatch. No tracing — these are plain CLI paths.

**Files:**
- Create: `crates/mudd/src/tenant.rs`
- Modify: `crates/mudd/src/lib.rs`

**Interfaces:**
- Consumes: `Settings` (Task 7), `Catalog`/`TenantName` (Tasks 3–5), `ensure_tenant_dir`/`Scaffolded` (Task 6).
- Produces:
  - `pub fn add(settings: &Settings, name: &str, out: &mut dyn Write) -> anyhow::Result<()>`
  - `pub fn remove(settings: &Settings, name: &str, purge: bool, input: &mut dyn BufRead, out: &mut dyn Write) -> anyhow::Result<()>`
  - `pub fn list(settings: &Settings, out: &mut dyn Write) -> anyhow::Result<()>`

- [ ] **Step 1: Create the module and write the failing tests**

Create `crates/mudd/src/tenant.rs`:

```rust
//! The `mudd tenant` subcommand implementations: catalogue mutation plus
//! folder scaffolding, with injected IO for testability.

use std::io::{BufRead, Write};

use anyhow::Context;

use crate::catalog::{Catalog, TenantName};
use crate::config::Settings;
use crate::scaffold::{Scaffolded, ensure_tenant_dir};

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::net::IpAddr;
    use std::path::Path;

    use mud_net::{Burst, SustainedRate};

    use super::*;
    use crate::config::LogFormat;

    fn settings(root: &Path) -> Settings {
        Settings {
            rate: SustainedRate::DEFAULT,
            burst: Burst::DEFAULT,
            log_format: LogFormat::Text,
            tenants_dir: root.join("tenants"),
            bind: "127.0.0.1".parse::<IpAddr>().expect("ip"),
            base_port: 4000,
            catalog_path: root.join("catalog.toml"),
        }
    }

    fn output_of(buffer: &[u8]) -> &str {
        std::str::from_utf8(buffer).expect("command output is UTF-8")
    }

    #[test]
    fn add_scaffolds_registers_and_reports() {
        let root = tempfile::tempdir().expect("temp dir");
        let settings = settings(root.path());
        let mut out = Vec::new();

        add(&settings, "mygame", &mut out).expect("add succeeds");

        let catalog = Catalog::load(&settings.catalog_path).expect("catalog loads");
        assert_eq!(catalog.entries().len(), 1);
        assert!(settings.tenants_dir.join("mygame/config.toml").exists());
        let report = output_of(&out);
        assert!(report.contains("mygame"), "report names the tenant: {report}");
        assert!(report.contains("4000"), "report shows the port: {report}");
    }

    #[test]
    fn add_rejects_an_invalid_name_without_touching_disk() {
        let root = tempfile::tempdir().expect("temp dir");
        let settings = settings(root.path());
        let mut out = Vec::new();

        assert!(add(&settings, "My Game", &mut out).is_err());
        assert!(!settings.catalog_path.exists(), "catalogue must be untouched");
    }

    #[test]
    fn add_registers_an_existing_folder_without_overwriting() {
        let root = tempfile::tempdir().expect("temp dir");
        let settings = settings(root.path());
        let dir = settings.tenants_dir.join("mygame");
        std::fs::create_dir_all(&dir).expect("create dir");
        std::fs::write(dir.join("config.toml"), "start_room = \"town_square\"\n")
            .expect("write config");
        let mut out = Vec::new();

        add(&settings, "mygame", &mut out).expect("re-add succeeds");

        let config = std::fs::read_to_string(dir.join("config.toml")).expect("read back");
        assert_eq!(config, "start_room = \"town_square\"\n");
    }

    #[test]
    fn remove_frees_the_entry_and_keeps_the_folder() {
        let root = tempfile::tempdir().expect("temp dir");
        let settings = settings(root.path());
        let mut out = Vec::new();
        add(&settings, "mygame", &mut out).expect("add succeeds");

        let mut input = Cursor::new(Vec::new());
        remove(&settings, "mygame", false, &mut input, &mut out).expect("remove succeeds");

        let catalog = Catalog::load(&settings.catalog_path).expect("catalog loads");
        assert!(catalog.entries().is_empty());
        assert!(
            settings.tenants_dir.join("mygame/config.toml").exists(),
            "the folder survives a plain remove"
        );
    }

    #[test]
    fn purge_with_the_wrong_confirmation_changes_nothing() {
        let root = tempfile::tempdir().expect("temp dir");
        let settings = settings(root.path());
        let mut out = Vec::new();
        add(&settings, "mygame", &mut out).expect("add succeeds");

        let mut input = Cursor::new(b"wrong-name\n".to_vec());
        let result = remove(&settings, "mygame", true, &mut input, &mut out);

        assert!(result.is_err(), "mismatched confirmation must abort");
        let catalog = Catalog::load(&settings.catalog_path).expect("catalog loads");
        assert_eq!(catalog.entries().len(), 1, "catalogue must be untouched");
        assert!(settings.tenants_dir.join("mygame").exists());
    }

    #[test]
    fn purge_with_the_right_confirmation_deletes_the_folder() {
        let root = tempfile::tempdir().expect("temp dir");
        let settings = settings(root.path());
        let mut out = Vec::new();
        add(&settings, "mygame", &mut out).expect("add succeeds");

        let mut input = Cursor::new(b"mygame\n".to_vec());
        remove(&settings, "mygame", true, &mut input, &mut out).expect("purge succeeds");

        let catalog = Catalog::load(&settings.catalog_path).expect("catalog loads");
        assert!(catalog.entries().is_empty());
        assert!(!settings.tenants_dir.join("mygame").exists(), "folder deleted");
    }

    #[test]
    fn list_prints_one_row_per_tenant() {
        let root = tempfile::tempdir().expect("temp dir");
        let settings = settings(root.path());
        let mut out = Vec::new();
        add(&settings, "alpha", &mut out).expect("add alpha");
        add(&settings, "beta", &mut out).expect("add beta");

        let mut listing = Vec::new();
        list(&settings, &mut listing).expect("list succeeds");

        let text = output_of(&listing);
        assert!(text.contains("alpha") && text.contains("beta"), "both rows: {text}");
        assert!(text.contains("4000") && text.contains("4001"), "ports shown: {text}");
    }

    #[test]
    fn list_reports_an_empty_catalogue() {
        let root = tempfile::tempdir().expect("temp dir");
        let settings = settings(root.path());

        let mut listing = Vec::new();
        list(&settings, &mut listing).expect("list succeeds");

        assert!(output_of(&listing).contains("no tenants"));
    }
}
```

Register in `crates/mudd/src/lib.rs`:

```rust
pub mod tenant;
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p mudd tenant`
Expected: FAIL — `add`/`remove`/`list` not defined.

- [ ] **Step 3: Implement the commands**

Add to `crates/mudd/src/tenant.rs` (above `mod tests`):

```rust
/// Registers a tenant: parses the name, assigns a port and tag, scaffolds
/// (or re-registers) the folder, saves the catalogue, and reports the
/// assignment on `out`.
///
/// # Errors
///
/// Returns an error on an invalid name, a name already registered, an
/// exhausted port/tag space, a half-formed existing folder, or any
/// filesystem failure. The catalogue file is only written after the folder
/// exists, so a failed scaffold leaves the catalogue untouched.
pub fn add(settings: &Settings, name: &str, out: &mut dyn Write) -> anyhow::Result<()> {
    let name = TenantName::parse(name)?;
    let mut catalog = Catalog::load(&settings.catalog_path)?;
    let entry = catalog.add(name.clone(), settings.base_port)?;
    let dir = settings.tenants_dir.join(name.as_str());
    let scaffolded = ensure_tenant_dir(&dir, &name)?;
    catalog.save(&settings.catalog_path)?;

    let verb = match scaffolded {
        Scaffolded::Created => "created",
        Scaffolded::Registered => "registered existing folder",
    };
    writeln!(
        out,
        "added tenant {name}: port {port}, tag {tag}, {verb} {dir}",
        port = entry.port,
        tag = entry.tag.get(),
        dir = dir.display(),
    )
    .context("writing command output")?;
    Ok(())
}

/// Deregisters a tenant, freeing its port and tag. With `purge`, also
/// deletes the tenant folder — after an interactive confirmation on
/// `input` that must re-type the tenant name exactly.
///
/// # Errors
///
/// Returns an error on an invalid or unknown name, a mismatched purge
/// confirmation (nothing is changed), or any filesystem failure.
pub fn remove(
    settings: &Settings,
    name: &str,
    purge: bool,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    let name = TenantName::parse(name)?;
    let mut catalog = Catalog::load(&settings.catalog_path)?;
    let entry = catalog.remove(&name)?;
    let dir = settings.tenants_dir.join(name.as_str());

    if purge {
        writeln!(
            out,
            "This permanently deletes {dir}. Type the tenant name to confirm:",
            dir = dir.display(),
        )
        .context("writing confirmation prompt")?;
        let mut line = String::new();
        input.read_line(&mut line).context("reading confirmation")?;
        if line.trim() != name.as_str() {
            anyhow::bail!("confirmation did not match {name}; nothing was changed");
        }
    }

    catalog.save(&settings.catalog_path)?;
    if purge {
        std::fs::remove_dir_all(&dir)
            .with_context(|| format!("deleting tenant folder {}", dir.display()))?;
    }
    writeln!(
        out,
        "removed tenant {name}: port {port} and tag {tag} are free for reuse",
        port = entry.port,
        tag = entry.tag.get(),
    )
    .context("writing command output")?;
    Ok(())
}

/// Prints one row per registered tenant: name, port, tag, and derived
/// directory.
///
/// # Errors
///
/// Returns an error if the catalogue is unreadable or the write to `out`
/// fails.
pub fn list(settings: &Settings, out: &mut dyn Write) -> anyhow::Result<()> {
    let catalog = Catalog::load(&settings.catalog_path)?;
    if catalog.entries().is_empty() {
        writeln!(out, "no tenants registered: run `mudd tenant add <name>`")
            .context("writing command output")?;
        return Ok(());
    }
    for entry in catalog.entries() {
        writeln!(
            out,
            "{name}  port={port}  tag={tag}  dir={dir}",
            name = entry.name,
            port = entry.port,
            tag = entry.tag.get(),
            dir = settings.tenants_dir.join(entry.name.as_str()).display(),
        )
        .context("writing command output")?;
    }
    Ok(())
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p mudd tenant`
Expected: PASS (9 tests).

- [ ] **Step 5: Commit**

```bash
jj commit -m "feat(mudd): tenant add/remove/list command implementations" crates/mudd
```

---

### Task 9: CLI cutover — subcommands, dispatch, catalogue-backed serve

Replaces the flat `Cli` with subcommands, deletes `ServerConfig::resolve`
and `[[tenants]]` parsing, and wires `main` to the new paths. `ServerConfig`
survives as the boot input (`{ rate, burst, tenants, log_format }`), built
by the serve path.

**Files:**
- Modify: `crates/mudd/src/config.rs`
- Modify: `crates/mudd/src/main.rs`
- Modify: `crates/mudd/src/lib.rs`

**Interfaces:**
- Consumes: everything from Tasks 1–8.
- Produces:
  - `pub struct Cli { pub config: Option<PathBuf>, pub command: Command }` (clap, `--config` global, `arg_required_else_help`)
  - `pub enum Command { Serve(ServeArgs), Tenant(TenantCommand) }`
  - `pub struct ServeArgs { pub tenant_dir: Option<PathBuf>, pub listen: Option<SocketAddr>, pub rate: Option<NonZeroU32>, pub burst: Option<NonZeroU32>, pub log_format: Option<LogFormat> }`
  - `pub enum TenantCommand { Add { name: String }, Remove { name: String, purge: bool }, List }`
  - `pub fn serve_tenants(settings: &Settings, args: &ServeArgs) -> anyhow::Result<Vec<TenantEntry>>`

- [ ] **Step 1: Write the failing CLI parse tests**

Append to `mod tests` in `crates/mudd/src/config.rs`:

```rust
    use clap::Parser as _;

    #[test]
    fn bare_mudd_asks_for_a_subcommand() {
        let error = Cli::try_parse_from(["mudd"]).expect_err("bare mudd must not serve");
        assert_eq!(
            error.kind(),
            clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
    }

    #[test]
    fn config_is_a_global_flag() {
        let cli = Cli::try_parse_from(["mudd", "tenant", "list", "--config", "/etc/f.toml"])
            .expect("global --config parses after the subcommand");
        assert_eq!(cli.config, Some(PathBuf::from("/etc/f.toml")));
        assert!(matches!(cli.command, Command::Tenant(TenantCommand::List)));
    }

    #[test]
    fn serve_accepts_the_dev_flags() {
        let cli = Cli::try_parse_from([
            "mudd", "serve", "--tenant-dir", "/t", "--listen", "127.0.0.1:5000",
        ])
        .expect("serve flags parse");
        let Command::Serve(args) = cli.command else {
            panic!("expected the serve subcommand");
        };
        assert_eq!(args.tenant_dir, Some(PathBuf::from("/t")));
        assert_eq!(args.listen, Some("127.0.0.1:5000".parse().expect("addr")));
    }

    #[test]
    fn tenant_remove_takes_a_purge_flag() {
        let cli = Cli::try_parse_from(["mudd", "tenant", "remove", "old", "--purge"])
            .expect("remove parses");
        assert!(matches!(
            cli.command,
            Command::Tenant(TenantCommand::Remove { ref name, purge: true }) if name == "old"
        ));
    }

    #[test]
    fn tenant_dir_bypasses_the_catalogue() {
        figment::Jail::expect_with(|jail| {
            jail.set_env("XDG_CONFIG_HOME", jail.directory().display());
            jail.set_env("XDG_DATA_HOME", jail.directory().display());
            let settings =
                Settings::resolve(None, &Overrides::default()).expect("settings resolve");
            let args = ServeArgs {
                tenant_dir: Some(PathBuf::from("/t")),
                listen: None,
                rate: None,
                burst: None,
                log_format: None,
            };

            let tenants = serve_tenants(&settings, &args).expect("dev mode needs no catalogue");
            assert_eq!(
                tenants,
                vec![TenantEntry {
                    dir: PathBuf::from("/t"),
                    listen: DEFAULT_LISTEN,
                    tag: TenantTag::default(),
                }]
            );
            Ok(())
        });
    }
```

(`panic!` in a `#[cfg(test)]` module is allowed: the workspace `clippy.toml`
sets `allow-panic-in-tests = true`.)

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p mudd config`
Expected: FAIL — `Command`, `ServeArgs`, `TenantCommand`, `serve_tenants` not defined.

- [ ] **Step 3: Replace the flat CLI**

In `crates/mudd/src/config.rs`:

1. Delete the old flat `Cli` struct, `ServerConfig::resolve`,
   `RawServerConfig`, `RawTenantEntry`, and every old
   test that exercised them (`defaults_apply_when_the_config_file_is_absent`,
   `the_registry_loads_from_the_server_config`,
   `tenant_dir_flag_replaces_the_registry`,
   `env_overrides_file_and_flags_override_env`, the three `log_format` file/env
   tests, `an_unknown_log_format_is_a_startup_error`,
   `an_empty_registry_without_tenant_dir_is_an_error`,
   `duplicate_listen_addresses_are_rejected`, and the `cli_with_tenant_dir`
   helper). Their coverage moved to the `Settings` tests (Task 7) and the
   new parse tests. Keep `default_config_path` (used by `Settings::resolve`).
2. Add an equivalent `log_format` guard to the `Settings` tests if Task 7
   did not already cover it (it did: `settings_read_the_file_and_env_overrides_it`
   asserts `LogFormat::Json` from the file). Add one unknown-value test:

```rust
    #[test]
    fn an_unknown_log_format_is_a_startup_error() {
        figment::Jail::expect_with(|jail| {
            jail.create_file("config.toml", "log_format = \"yaml\"")?;
            let config_path = jail.directory().join("config.toml");
            let result = Settings::resolve(Some(&config_path), &Overrides::default());
            assert!(result.is_err(), "unknown log format must fail fast, got {result:?}");
            Ok(())
        });
    }
```

3. Define the new CLI:

```rust
/// Command-line interface for the `mudd` server binary.
#[derive(Debug, clap::Parser)]
#[command(name = "mudd", version, about = "The Ferrodun MUD server", arg_required_else_help = true)]
pub struct Cli {
    /// Server config path (default: $XDG_CONFIG_HOME/ferrodun/config.toml).
    /// The tenant catalogue (catalog.toml) always sits beside it.
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Command,
}

/// Top-level `mudd` subcommands.
#[derive(Debug, clap::Subcommand)]
pub enum Command {
    /// Serve every tenant registered in the catalogue.
    Serve(ServeArgs),
    /// Manage the tenant catalogue.
    #[command(subcommand)]
    Tenant(TenantCommand),
}

/// Flags for `mudd serve`; each overrides the server config.
#[derive(Debug, clap::Args)]
pub struct ServeArgs {
    /// Boot exactly this tenant, bypassing the catalogue (dev mode).
    #[arg(long)]
    pub tenant_dir: Option<PathBuf>,
    /// Listen address for --tenant-dir mode (default 127.0.0.1:4000).
    #[arg(long)]
    pub listen: Option<SocketAddr>,
    /// Per-session sustained command rate (commands/second).
    #[arg(long)]
    pub rate: Option<NonZeroU32>,
    /// Per-session command burst allowance.
    #[arg(long)]
    pub burst: Option<NonZeroU32>,
    /// Log wire format: `text` (default) or `json`.
    #[arg(long, value_enum)]
    pub log_format: Option<LogFormat>,
}

/// `mudd tenant` subcommands.
#[derive(Debug, clap::Subcommand)]
pub enum TenantCommand {
    /// Register a tenant: assign a port and tag, scaffold its folder.
    Add { name: String },
    /// Deregister a tenant. --purge also deletes its folder (asks for
    /// confirmation).
    Remove {
        name: String,
        #[arg(long)]
        purge: bool,
    },
    /// List registered tenants.
    List,
}

/// Builds the boot registry for `mudd serve`: the single `--tenant-dir`
/// tenant (tag 0) in dev mode, otherwise the catalogue via
/// [`tenants_from_catalog`].
///
/// # Errors
///
/// Returns an error if the catalogue is unreadable, invalid, or empty.
pub fn serve_tenants(settings: &Settings, args: &ServeArgs) -> anyhow::Result<Vec<TenantEntry>> {
    match &args.tenant_dir {
        Some(dir) => Ok(vec![TenantEntry {
            dir: dir.clone(),
            listen: args.listen.unwrap_or(DEFAULT_LISTEN),
            tag: TenantTag::default(),
        }]),
        None => {
            let catalog = Catalog::load(&settings.catalog_path)?;
            tenants_from_catalog(settings, &catalog)
        }
    }
}
```

4. `ServerConfig` stays exactly as it is (`{ rate, burst, tenants, log_format }`,
   the boot input used by integration tests) — only its `resolve` method is
   gone.
5. In the tests module, add the imports the new tests need
   (`use mud_core::TenantTag;` is already there from Task 1).

Update `crates/mudd/src/lib.rs` exports:

```rust
pub use config::{
    Cli, Command, LogFormat, Overrides, ServeArgs, ServerConfig, Settings, TenantCommand,
    TenantEntry, serve_tenants, tenants_from_catalog,
};
```

6. Rewrite `crates/mudd/src/main.rs`:

```rust
use std::path::Path;

use anyhow::Context;
use clap::Parser;
use mudd::config::{Cli, Command, LogFormat, Overrides, ServeArgs, ServerConfig, Settings, TenantCommand};
use mudd::{serve_tenants, tenant};
use tracing_subscriber::EnvFilter;

/// Installs the process-global subscriber. JSON mode emits current-span and
/// span-list fields so the tenant/session/command span taxonomy (design §4)
/// is visible to aggregators; the text formatter shows spans in its prefix.
fn init_tracing(format: LogFormat) {
    let filter = EnvFilter::try_from_env("RUST_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    match format {
        LogFormat::Text => tracing_subscriber::fmt().with_env_filter(filter).init(),
        LogFormat::Json => tracing_subscriber::fmt()
            .with_env_filter(filter)
            .json()
            .with_current_span(true)
            .with_span_list(true)
            .init(),
    }
}

/// Entry point for the `mudd` server binary: dispatches `serve` and
/// `tenant` subcommands. Bare `mudd` prints the subcommand help (clap
/// `arg_required_else_help`).
fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Serve(args) => serve(cli.config.as_deref(), &args),
        Command::Tenant(command) => run_tenant(cli.config.as_deref(), command),
    }
}

/// Resolves settings, builds the boot registry (catalogue or
/// `--tenant-dir`), and serves until shutdown. Fail-stop: any tenant task
/// fault or a panicked task ends the process (design §8).
fn serve(config: Option<&Path>, args: &ServeArgs) -> anyhow::Result<()> {
    // Resolve config before installing the subscriber so the log format is
    // itself a configured value (flag > MUDD_ env > config.toml > default).
    // Nothing logs before `boot`, so a config error surfacing here without a
    // tracing subscriber loses no diagnostics.
    let overrides = Overrides {
        rate: args.rate,
        burst: args.burst,
        log_format: args.log_format,
    };
    let settings = Settings::resolve(config, &overrides)?;
    init_tracing(settings.log_format);

    let tenants = serve_tenants(&settings, args)?;
    let server = ServerConfig {
        rate: settings.rate,
        burst: settings.burst,
        tenants,
        log_format: settings.log_format,
    };

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("building tokio runtime")?;
    runtime.block_on(async_main(server))
}

/// Dispatches a `mudd tenant` subcommand against the catalogue.
fn run_tenant(config: Option<&Path>, command: TenantCommand) -> anyhow::Result<()> {
    let settings = Settings::resolve(config, &Overrides::default())?;
    let mut out = std::io::stdout().lock();
    match command {
        TenantCommand::Add { name } => tenant::add(&settings, &name, &mut out),
        TenantCommand::Remove { name, purge } => {
            let mut input = std::io::stdin().lock();
            tenant::remove(&settings, &name, purge, &mut input, &mut out)
        }
        TenantCommand::List => tenant::list(&settings, &mut out),
    }
}
```

Keep `async_main` exactly as it is today (below `run_tenant`).

- [ ] **Step 4: Run the full workspace verification**

Run: `cargo test --workspace`
Expected: PASS.

Run: `cargo clippy --workspace --all-targets`
Expected: clean (no warnings).

- [ ] **Step 5: Smoke-test the binary by hand**

```bash
target_dir=$(mktemp -d)
XDG_CONFIG_HOME="$target_dir/config" XDG_DATA_HOME="$target_dir/data" cargo run -p mudd -- tenant add demo
XDG_CONFIG_HOME="$target_dir/config" XDG_DATA_HOME="$target_dir/data" cargo run -p mudd -- tenant list
cargo run -p mudd 2>&1 | head -5
```

Expected: `add` prints `added tenant demo: port 4000, tag 1, created …/data/ferrodun/tenants/demo`; `list` shows the row; bare `mudd` prints the help synopsis with `serve` and `tenant`.

- [ ] **Step 6: Commit**

```bash
jj commit -m "feat(mudd): subcommand CLI - serve + tenant add/remove/list" crates/mudd
```

---

### Task 10: Documentation, PLAN, and JOURNAL

**Files:**
- Modify: `docs/docs/operating/configuration.md`
- Modify: `docs/docs/operating/running-a-server.md`
- Modify: `PLAN.md`
- Modify: `.claude/JOURNAL.md`

- [ ] **Step 1: Rewrite `docs/docs/operating/configuration.md`**

Replace the whole file with:

```markdown
# Configuration

The full key reference for `mudd`'s server-wide config file, the tenant
catalogue, and each tenant's `config.toml`.

## Server-wide configuration

`mudd` reads a server-wide configuration file from
`$XDG_CONFIG_HOME/ferrodun/config.toml` (by default
`~/.config/ferrodun/config.toml`). Override the location with the global
`--config` flag. Every key is optional:

```toml
rate = 10           # per-session sustained commands/second
burst = 20          # per-session burst allowance
log_format = "text" # log wire format: "text" (default) or "json"
bind = "127.0.0.1"  # host address every tenant listener binds to
base_port = 4000    # lowest port the catalogue may assign
tenants_dir = "/srv/ferrodun/tenants" # root holding one folder per tenant
```

| Key | Default | Meaning |
|---|---|---|
| `rate` | `10` | Per-session sustained commands/second. |
| `burst` | `20` | Per-session burst allowance. |
| `log_format` | `text` | Log wire format: `text` or `json`. Also settable via `--log-format` or `MUDD_LOG_FORMAT`. |
| `bind` | `127.0.0.1` | Host address every tenant listener binds to. Set `0.0.0.0` to expose publicly. |
| `base_port` | `4000` | Lowest port `mudd tenant add` may assign. |
| `tenants_dir` | `$XDG_DATA_HOME/ferrodun/tenants` | Root directory holding one folder per tenant, named after it. |

Configuration is layered, weakest first:

1. built-in defaults,
2. `config.toml`,
3. `MUDD_*` environment variables (e.g. `MUDD_RATE=5`),
4. command-line flags.

## The tenant catalogue

The tenant registry lives in `catalog.toml`, a sibling of the server config
file. It is **machine-managed**: `mudd tenant add` and `mudd tenant remove`
are its writers, and there are no environment or flag overrides for its
contents. Each entry records the tenant's name and its assigned values:

```toml
[[tenants]]
name = "midgard"
port = 4000
tag = 1
```

- The tenant's directory is always `<tenants_dir>/<name>` — no path is
  stored.
- `port` is assigned by `mudd tenant add`: the lowest free port at or above
  `base_port`. Ports freed by `mudd tenant remove` are reused.
- `tag` is the runtime tenant tag stamped into the tenant's entity ids
  (12-bit, `1..=4095`; `0` is reserved for `--tenant-dir` dev mode).
  Assigned lowest-free, reused after removal.

Hand-edits are validated when the file loads: names, ports, and tags must
be unique, and tags must be in range. See
[Running a server](running-a-server.md) for the `mudd tenant` commands.

## Per-tenant configuration

Inside a tenant directory, `config.toml` describes that one world. It is
builder content only — no ports, no tags. There is no environment-variable
override for these keys — the file is the sole source of a tenant's
configuration.

| Key | Required | Default | Meaning |
|---|---|---|---|
| `start_room` | yes | — | Slug of the room new characters begin in. |
| `locale` | no | `"en"` | Language engine messages render in. See [Localization](../building/localization.md). |
| `banner` | no | `welcome.kdl` | Welcome-banner file, relative to the tenant directory. |
| `palette` | no | `palette.kdl` | Color palette file, relative to the tenant directory. |
```

- [ ] **Step 2: Update `docs/docs/operating/running-a-server.md`**

Replace the `## Quick start` section with:

```markdown
## Quick start

Register a tenant, then serve:

```
mudd tenant add mygame
mudd serve
```

`tenant add` scaffolds a minimal bootable world under the tenants directory
(see [Configuration](configuration.md)) and assigns the tenant a port
(starting at 4000) — the moment it finishes, `mudd serve` boots the tenant
and a player can connect. Manage the registry with `mudd tenant list` and
`mudd tenant remove <name>` (add `--purge` to also delete the folder;
without it, the folder and its database stay on disk).

For a one-off world in a specific folder, bypass the catalogue:

```
mudd serve --tenant-dir /path/to/tenant
```

This serves that tenant over telnet on `127.0.0.1:4000`. The tenant directory
holds the world's `config.toml`, its `world/` room files, and its welcome
banner — see [World files](../building/world-files.md).

Running `mudd` with no subcommand prints the available commands.

See [Configuration](configuration.md) for every setting.
```

In the systemd unit example, change `ExecStart=/usr/local/bin/mudd` to
`ExecStart=/usr/local/bin/mudd serve`.

- [ ] **Step 3: Verify the docs build**

Run (from `docs/`): `uv run mkdocs build --strict`
Expected: build succeeds with no warnings.

- [ ] **Step 4: Add the PLAN.md entry**

Append after the `M1-23` entry in `PLAN.md`:

```markdown
- **M1-24 — Tenant catalogue + `mudd` subcommand CLI.** Remove `tenant_tag`
  from tenant `config.toml` (restoring M1-12's "content fields only"
  contract); introduce the operator-side tenant catalogue (`catalog.toml`,
  sibling of the server config) that assigns each tenant its port (lowest
  free ≥ `base_port`, reused after removal) and its runtime tag (lowest
  free ≥ 1; 0 stays the `--tenant-dir` dev tag); restructure `mudd` into
  subcommands — `serve`, `tenant add/remove/list` — with bare `mudd`
  printing help and `--config` global. `[[tenants]]` leaves the server
  config in favor of the catalogue; new server keys `tenants_dir`, `bind`,
  `base_port`. The duplicate-tag and duplicate-listen boot errors disappear
  (uniqueness is the catalogue's invariant, by construction).
  - *Spec:* §3.11.3; design doc
    `docs/superpowers/specs/2026-07-11-tenant-catalog-cli-design.md`.
    *Verify:* `mudd tenant add` scaffolds a tenant that `mudd serve` boots;
    workspace tests and clippy green.
```

- [ ] **Step 5: Add the JOURNAL entry**

Append to `.claude/JOURNAL.md`:

```markdown
## 2026-07-11 — Tenant catalogue + mudd subcommand CLI (M1-24)

- **Spec:** §3.11.3; docs/superpowers/specs/2026-07-11-tenant-catalog-cli-design.md
- **Done:** `tenant_tag` removed from tenant config (runtime tag now rides
  `TenantEntry`, assigned by the new `catalog.toml` next to the server
  config); `mudd` restructured into `serve` + `tenant add/remove/list`
  subcommands (bare `mudd` = help, `--config` global); server config gained
  `tenants_dir`/`bind`/`base_port`, lost `[[tenants]]`; `tenant add`
  scaffolds a minimal bootable world.
- **Verify:** `cargo test --workspace`, `cargo clippy --workspace
  --all-targets`, `uv run mkdocs build --strict`; manual `tenant
  add`/`list`/bare-`mudd` smoke test.
- **Next:** none — catalogue and CLI are complete per the design doc.
```

- [ ] **Step 6: Commit**

```bash
jj commit -m "docs: catalogue-era configuration and server docs, PLAN M1-24, journal" docs/docs PLAN.md .claude/JOURNAL.md
```

---

## Final verification (after all tasks)

- [ ] `cargo test --workspace` — all green.
- [ ] `cargo clippy --workspace --all-targets` — no warnings.
- [ ] `cd docs && uv run mkdocs build --strict` — clean build.
- [ ] `rg -n "tenant_tag" crates/ docs/docs/` — no hits left outside this plan's history (SPEC/PLAN mentions of the *EntityId tenant tag* concept are expected and stay).
- [ ] Manual smoke test from Task 9 Step 5 repeated once from a clean temp HOME.
