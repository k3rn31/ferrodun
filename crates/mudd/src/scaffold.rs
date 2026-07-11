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
    // config.toml is written last: its presence is what ensure_tenant_dir
    // reads to decide a folder is a complete tenant. Writing it only after
    // the world files means an interrupted scaffold leaves no config.toml,
    // so the next call rejects the half-formed folder instead of registering it.
    write_new(&dir.join("config.toml"), "start_room = \"start\"\n")?;
    Ok(Scaffolded::Created)
}

/// Writes a scaffold file, reporting the path on failure.
fn write_new(path: &Path, contents: &str) -> anyhow::Result<()> {
    std::fs::write(path, contents)
        .with_context(|| format!("writing scaffold file {}", path.display()))
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
        let _world = load_world(&config).expect("scaffolded world loads");
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
        assert!(
            !dir.join("world").exists(),
            "no scaffold on an existing dir"
        );
    }

    #[test]
    fn a_dir_without_config_is_an_error() {
        let root = tempfile::tempdir().expect("temp dir");
        let dir = root.path().join("mygame");
        std::fs::create_dir_all(&dir).expect("create dir");

        assert!(ensure_tenant_dir(&dir, &name("mygame")).is_err());
    }
}
