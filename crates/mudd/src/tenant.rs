//! The `mudd tenant` subcommand implementations: catalogue mutation plus
//! folder scaffolding, with injected IO for testability.

use std::io::{BufRead, Write};

use anyhow::Context;

use crate::catalog::{Catalog, TenantName};
use crate::config::Settings;
use crate::scaffold::{Scaffolded, ensure_tenant_dir};

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
