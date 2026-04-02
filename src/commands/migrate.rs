use std::fs;

use anyhow::Context;

use crate::project::find_project_root;
use crate::DynResult;

/// Migrate a scaffold.toml from old lssa naming to new lez naming.
/// Also updates the URL from lssa.git to logos-execution-zone.git.
pub(crate) fn cmd_migrate() -> DynResult<()> {
    let cwd = std::env::current_dir()?;
    let root = find_project_root(cwd.clone()).ok_or_else(|| {
        anyhow::anyhow!(
            "Not a logos-scaffold project at {}. Run `logos-scaffold create <name>` first.",
            cwd.display()
        )
    })?;

    let config_path = root.join("scaffold.toml");
    let raw = fs::read_to_string(&config_path)
        .with_context(|| format!("Failed to read {}", config_path.display()))?;

    // Check if migration is needed
    if !needs_migration(&raw) {
        println!("✅ scaffold.toml is already up to date — no migration needed.");
        return Ok(());
    }

    println!("🔄 Migrating scaffold.toml...");

    let migrated = migrate_config(&raw);

    // Show a diff summary
    let old_lssa_section = raw.lines()
        .filter(|l| l.contains("lssa") || l.contains("logos-blockchain/lssa"))
        .collect::<Vec<_>>();
    let new_lez_section = migrated.lines()
        .filter(|l| l.contains("lez") || l.contains("logos-execution-zone"))
        .collect::<Vec<_>>();

    println!("\nChanges:");
    for (old, new) in old_lssa_section.iter().zip(new_lez_section.iter()) {
        if old != new {
            println!("  - {}", old.trim());
            println!("  + {}", new.trim());
        }
    }

    // Write the migrated config
    fs::write(&config_path, &migrated)
        .with_context(|| format!("Failed to write {}", config_path.display()))?;

    println!("\n✅ Migration complete. scaffold.toml updated.");
    println!("   Run `logos-scaffold doctor` to verify the configuration.");

    Ok(())
}

/// Returns true if the config needs migration (has old lssa keys or URLs).
pub(crate) fn needs_migration(raw: &str) -> bool {
    raw.contains("[repos.lssa]")
        || raw.contains("logos-blockchain/lssa.git")
        || raw.contains("logos-blockchain/lssa\"")
}

/// Apply all migrations to the raw config text.
fn migrate_config(raw: &str) -> String {
    raw
        // Rename section header
        .replace("[repos.lssa]", "[repos.lez]")
        // Update old lssa.git URL to logos-execution-zone
        .replace(
            "https://github.com/logos-blockchain/lssa.git",
            "https://github.com/logos-blockchain/logos-execution-zone.git",
        )
}
