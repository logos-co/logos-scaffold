use std::env;
use std::fs;
use std::path::Path;

use anyhow::{bail, Context};
use toml_edit::{value, DocumentMut, Item, Table};

use crate::config::{
    default_basecamp_repo, default_lez_repo, default_lgpm_repo, default_spel_repo,
    serialize_config, split_flake_ref,
};
use crate::constants::{
    BASECAMP_ATTR, BASECAMP_SOURCE, DEFAULT_BASECAMP_PIN, DEFAULT_FRAMEWORK_IDL_PATH,
    DEFAULT_FRAMEWORK_IDL_SPEC, DEFAULT_FRAMEWORK_VERSION, DEFAULT_LEZ, DEFAULT_LGPM_PIN,
    DEFAULT_SPEL, FRAMEWORK_KIND_DEFAULT, LGPM_ATTR, LGPM_SOURCE, SCAFFOLD_TOML_SCHEMA_VERSION,
};
use crate::model::{Config, FrameworkConfig, FrameworkIdlConfig, LocalnetConfig};
use crate::state::write_text;
use crate::template::project::ensure_scaffold_in_gitignore;
use crate::DynResult;

pub(crate) fn cmd_init(bin_name: &str) -> DynResult<()> {
    let cwd = env::current_dir()?;
    cmd_init_at(&cwd, bin_name)
}

pub(crate) fn cmd_init_at(target: &Path, bin_name: &str) -> DynResult<()> {
    let scaffold_path = target.join("scaffold.toml");
    if scaffold_path.exists() {
        let existing = fs::read_to_string(&scaffold_path).with_context(|| {
            format!(
                "reading existing scaffold.toml at {}",
                scaffold_path.display()
            )
        })?;
        let mut doc: DocumentMut = existing.parse().with_context(|| {
            format!(
                "parsing existing scaffold.toml at {}",
                scaffold_path.display()
            )
        })?;

        let report = migrate_to_v0_2_0(&mut doc)?;
        if report.changes.is_empty() {
            bail!(
                "scaffold.toml at {} is already at schema v{} — nothing to migrate",
                target.display(),
                SCAFFOLD_TOML_SCHEMA_VERSION,
            );
        }
        write_text(&scaffold_path, &doc.to_string())?;
        println!(
            "scaffold.toml in {} migrated to schema v{}.",
            target.display(),
            SCAFFOLD_TOML_SCHEMA_VERSION,
        );
        for change in report.changes {
            println!("  - {change}");
        }
        if let Some(hint) = report.hand_edit_hint {
            println!("  ! {hint}");
        }
        println!("Run `{bin_name} setup` to clone and build per the new schema.");
        return Ok(());
    }

    // Fresh init — schema 0.2.0 by construction.
    let cfg = fresh_default_config();
    write_text(&scaffold_path, &serialize_config(&cfg)?)?;
    fs::create_dir_all(target.join(".scaffold/state"))
        .with_context(|| format!("creating {}/.scaffold/state", target.display()))?;
    fs::create_dir_all(target.join(".scaffold/logs"))
        .with_context(|| format!("creating {}/.scaffold/logs", target.display()))?;
    ensure_scaffold_in_gitignore(target)?;

    println!(
        "scaffold.toml created at {}. Run '{bin_name} setup' to clone LEZ and build dependencies.",
        scaffold_path.display()
    );
    println!(
        "If this project is building modules for basecamp, run '{bin_name} basecamp setup' to pin + build basecamp + lgpm and seed alice/bob profiles."
    );

    Ok(())
}

fn fresh_default_config() -> Config {
    Config {
        version: SCAFFOLD_TOML_SCHEMA_VERSION.to_string(),
        cache_root: String::new(),
        lez: default_lez_repo(DEFAULT_LEZ.sha),
        spel: default_spel_repo(DEFAULT_SPEL.sha),
        basecamp_repo: Some(default_basecamp_repo(DEFAULT_BASECAMP_PIN)),
        lgpm_repo: Some(default_lgpm_repo(DEFAULT_LGPM_PIN)),
        wallet_home_dir: ".scaffold/wallet".to_string(),
        framework: FrameworkConfig {
            kind: FRAMEWORK_KIND_DEFAULT.to_string(),
            version: DEFAULT_FRAMEWORK_VERSION.to_string(),
            idl: FrameworkIdlConfig {
                spec: DEFAULT_FRAMEWORK_IDL_SPEC.to_string(),
                path: DEFAULT_FRAMEWORK_IDL_PATH.to_string(),
            },
        },
        localnet: LocalnetConfig::default(),
        modules: std::collections::BTreeMap::new(),
        basecamp: None,
    }
}

#[derive(Default)]
struct MigrationReport {
    changes: Vec<String>,
    /// Set when migration succeeded but a field was unparseable and the user
    /// must hand-edit. Currently only triggered by malformed `lgpm_flake`.
    hand_edit_hint: Option<String>,
}

/// Mutate `doc` in place from any pre-0.2.0 schema to v0.2.0. Preserves
/// comments, key ordering, and unrelated sections via toml_edit. Returns a
/// report listing what changed; an empty report means "already migrated."
///
/// The input may be:
/// - A pre-spel scaffold.toml (no `[repos.spel]` section). The original
///   migration's job — append the section.
/// - A 0.1.x-era scaffold.toml with `url` fields, `[basecamp].pin/.source/
///   .lgpm_flake`, or `[basecamp.modules.*]`. Reshape all of those.
/// - A mix of the above.
fn migrate_to_v0_2_0(doc: &mut DocumentMut) -> DynResult<MigrationReport> {
    let mut report = MigrationReport::default();

    // Ensure [scaffold] exists; bump version.
    let scaffold = doc.entry("scaffold").or_insert(Item::Table({
        let mut t = Table::new();
        t.set_implicit(false);
        t
    }));
    let scaffold_table = scaffold
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("[scaffold] is not a table"))?;
    let current_version = scaffold_table
        .get("version")
        .and_then(Item::as_str)
        .unwrap_or("")
        .to_string();
    if current_version != SCAFFOLD_TOML_SCHEMA_VERSION {
        scaffold_table["version"] = value(SCAFFOLD_TOML_SCHEMA_VERSION);
        report.changes.push(format!(
            "bumped [scaffold].version: {:?} -> {:?}",
            if current_version.is_empty() {
                "<unset>"
            } else {
                current_version.as_str()
            },
            SCAFFOLD_TOML_SCHEMA_VERSION,
        ));
    }

    // [repos.lssa] -> [repos.lez] alias rename. Only triggered if the
    // explicit lssa section exists; harmless otherwise.
    if let Some(repos) = doc.get_mut("repos").and_then(Item::as_table_mut) {
        if repos.contains_key("lssa") && !repos.contains_key("lez") {
            if let Some(lssa) = repos.remove("lssa") {
                repos.insert("lez", lssa);
                report
                    .changes
                    .push("renamed [repos.lssa] -> [repos.lez]".to_string());
            }
        }
    }

    // Drop `url` from [repos.lez] / [repos.spel].
    for name in ["lez", "spel"] {
        if let Some(repo) = doc
            .get_mut("repos")
            .and_then(Item::as_table_mut)
            .and_then(|r| r.get_mut(name).and_then(Item::as_table_mut))
        {
            if repo.remove("url").is_some() {
                report
                    .changes
                    .push(format!("removed [repos.{name}].url (use `source` only)"));
            }
        }
    }

    // Append [repos.spel] if missing (pre-spel migration semantics).
    let spel_missing = doc
        .get("repos")
        .and_then(Item::as_table)
        .and_then(|r| r.get("spel"))
        .is_none();
    if spel_missing {
        // Vendor-detection: if existing [repos.lez].path is `.scaffold/repos/lez`,
        // mirror the layout for spel. Otherwise leave path empty (portable).
        let lez_path = doc
            .get("repos")
            .and_then(Item::as_table)
            .and_then(|r| r.get("lez").and_then(Item::as_table))
            .and_then(|t| t.get("path").and_then(Item::as_str))
            .unwrap_or("")
            .to_string();
        let mut spel = default_spel_repo(DEFAULT_SPEL.sha);
        if lez_path == ".scaffold/repos/lez" {
            spel.path = ".scaffold/repos/spel".to_string();
        }
        write_repo_ref_via_toml_edit(doc, "spel", &spel);
        report
            .changes
            .push("appended [repos.spel] with default pin".to_string());
    }

    // Migrate [basecamp].pin / .source -> [repos.basecamp].
    let mut basecamp_pin = None;
    let mut basecamp_source = None;
    let mut lgpm_flake = None;
    if let Some(bc) = doc.get_mut("basecamp").and_then(Item::as_table_mut) {
        if let Some(s) = bc.get("pin").and_then(Item::as_str) {
            basecamp_pin = Some(s.to_string());
        }
        if let Some(s) = bc.get("source").and_then(Item::as_str) {
            basecamp_source = Some(s.to_string());
        }
        if let Some(s) = bc.get("lgpm_flake").and_then(Item::as_str) {
            lgpm_flake = Some(s.to_string());
        }
    }

    let need_basecamp_repo = basecamp_pin.is_some() || basecamp_source.is_some();
    if need_basecamp_repo {
        let pin = basecamp_pin
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_BASECAMP_PIN.to_string());
        let source = basecamp_source
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| BASECAMP_SOURCE.to_string());
        let mut repo = default_basecamp_repo(&pin);
        repo.source = source;
        repo.attr = BASECAMP_ATTR.to_string();
        write_repo_ref_via_toml_edit(doc, "basecamp", &repo);
        report
            .changes
            .push("migrated [basecamp].pin / .source -> [repos.basecamp]".to_string());
    }

    // Migrate [basecamp].lgpm_flake -> [repos.lgpm].
    if let Some(flake_ref) = lgpm_flake {
        if !flake_ref.is_empty() {
            match split_flake_ref(&flake_ref) {
                Some((source, pin, attr)) => {
                    let mut repo = default_lgpm_repo(&pin);
                    repo.source = source;
                    repo.attr = if attr.is_empty() {
                        LGPM_ATTR.to_string()
                    } else {
                        attr
                    };
                    write_repo_ref_via_toml_edit(doc, "lgpm", &repo);
                    report
                        .changes
                        .push("migrated [basecamp].lgpm_flake -> [repos.lgpm]".to_string());
                }
                None => {
                    // Unparseable — write a placeholder repo with default
                    // pin and tell the user to fix it by hand. We still
                    // strip the old key so the file ends up valid.
                    let repo = default_lgpm_repo(DEFAULT_LGPM_PIN);
                    write_repo_ref_via_toml_edit(doc, "lgpm", &repo);
                    report.hand_edit_hint = Some(format!(
                        "could not parse [basecamp].lgpm_flake = {flake_ref:?}; wrote default \
                         [repos.lgpm] (source={LGPM_SOURCE}, pin={DEFAULT_LGPM_PIN}). Edit \
                         scaffold.toml to set the right pin."
                    ));
                    report
                        .changes
                        .push("migrated [basecamp].lgpm_flake -> [repos.lgpm] (default pin; verify by hand)".to_string());
                }
            }
        }
    }

    // [basecamp.modules.*] -> [modules.*]
    let mut moved_modules = Vec::new();
    if let Some(bc) = doc.get_mut("basecamp").and_then(Item::as_table_mut) {
        if let Some(modules_item) = bc.get("modules") {
            if let Some(modules_table) = modules_item.as_table() {
                for (name, item) in modules_table.iter() {
                    if let Some(t) = item.as_table() {
                        moved_modules.push((name.to_string(), t.clone()));
                    }
                }
            }
        }
        bc.remove("modules");
    }
    if !moved_modules.is_empty() {
        let modules_root = doc.entry("modules").or_insert(Item::Table({
            let mut t = Table::new();
            t.set_implicit(true);
            t
        }));
        let modules_table = modules_root
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("[modules] is not a table"))?;
        for (name, t) in &moved_modules {
            modules_table.insert(name, Item::Table(t.clone()));
        }
        report.changes.push(format!(
            "moved [basecamp.modules.*] -> [modules.*] ({} entr{})",
            moved_modules.len(),
            if moved_modules.len() == 1 { "y" } else { "ies" },
        ));
    }

    // Strip migrated keys from [basecamp].
    if let Some(bc) = doc.get_mut("basecamp").and_then(Item::as_table_mut) {
        for stale in ["pin", "source", "lgpm_flake"] {
            bc.remove(stale);
        }
        // If [basecamp] is now empty (no port_base/port_stride either),
        // drop the section entirely.
        if bc.iter().next().is_none() {
            doc.as_table_mut().remove("basecamp");
        }
    }

    Ok(report)
}

fn write_repo_ref_via_toml_edit(doc: &mut DocumentMut, name: &str, repo: &crate::model::RepoRef) {
    let repos = doc.entry("repos").or_insert(Item::Table({
        let mut t = Table::new();
        t.set_implicit(true);
        t
    }));
    let repos_table = repos.as_table_mut().expect("repos is a table");
    repos_table.set_implicit(true);
    let table = repos_table
        .entry(name)
        .or_insert(Item::Table(Table::new()))
        .as_table_mut()
        .expect("repo is a table");
    table["source"] = value(&repo.source);
    table["pin"] = value(&repo.pin);
    if repo.build != crate::model::RepoBuild::default() {
        table["build"] = value(repo.build.as_str());
    } else {
        table.remove("build");
    }
    if !repo.attr.is_empty() {
        table["attr"] = value(&repo.attr);
    } else {
        table.remove("attr");
    }
    if !repo.path.is_empty() {
        table["path"] = value(&repo.path);
    } else {
        table.remove("path");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse_config;
    use crate::constants::{LEZ_SOURCE, SPEL_SOURCE};
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn init_writes_parseable_v0_2_0_scaffold_toml() {
        let temp = tempdir().expect("tempdir");
        let target = temp.path();
        cmd_init_at(target, "lgs").expect("init");

        let text = fs::read_to_string(target.join("scaffold.toml")).expect("read scaffold.toml");
        let cfg = parse_config(&text).expect("parse scaffold.toml");

        assert_eq!(cfg.version, SCAFFOLD_TOML_SCHEMA_VERSION);
        assert_eq!(cfg.lez.pin, DEFAULT_LEZ.sha);
        assert_eq!(cfg.spel.pin, DEFAULT_SPEL.sha);
        assert_eq!(cfg.framework.kind, FRAMEWORK_KIND_DEFAULT);
        assert_eq!(cfg.wallet_home_dir, ".scaffold/wallet");
        assert_eq!(cfg.localnet.port, 3040);
        assert!(cfg.localnet.risc0_dev_mode);
        let bc = cfg.basecamp_repo.expect("basecamp present");
        assert_eq!(bc.attr, BASECAMP_ATTR);
        assert_eq!(bc.build, crate::model::RepoBuild::NixFlake);
        let lgpm = cfg.lgpm_repo.expect("lgpm present");
        assert_eq!(lgpm.attr, LGPM_ATTR);
    }

    #[test]
    fn init_does_not_write_url_field() {
        let temp = tempdir().expect("tempdir");
        let target = temp.path();
        cmd_init_at(target, "lgs").expect("init");
        let text = fs::read_to_string(target.join("scaffold.toml")).expect("read");
        assert!(
            !text.contains("url ="),
            "v0.2.0 scaffold.toml must not contain url field; got:\n{text}"
        );
    }

    #[test]
    fn init_does_not_persist_cache_root() {
        let temp = tempdir().expect("tempdir");
        let target = temp.path();
        cmd_init_at(target, "lgs").expect("init");
        let text = fs::read_to_string(target.join("scaffold.toml")).expect("read");
        let has_active = text
            .lines()
            .any(|l| !l.trim_start().starts_with('#') && l.contains("cache_root"));
        assert!(
            !has_active,
            "scaffold.toml should not pin cache_root by default; got:\n{text}"
        );
    }

    #[test]
    fn init_refuses_when_already_at_v0_2_0() {
        let temp = tempdir().expect("tempdir");
        let target = temp.path();
        cmd_init_at(target, "lgs").expect("init");
        let err = cmd_init_at(target, "lgs").expect_err("should refuse");
        assert!(err.to_string().contains("already at schema"), "{err}");
    }

    #[test]
    fn migrates_pre_spel_scaffold_toml() {
        let temp = tempdir().expect("tempdir");
        let target = temp.path();
        // Pre-spel: no [repos.spel], legacy [repos.lez] with url field.
        let seed = r#"# user comment
[scaffold]
version = "0.1.0"

[repos.lez]
url = "https://example.com/lez.git"
source = "https://example.com/lez.git"
path = ""
pin = "abc"

[wallet]
home_dir = ".scaffold/wallet"
"#;
        fs::write(target.join("scaffold.toml"), seed).expect("seed");

        cmd_init_at(target, "lgs").expect("migrate");

        let after = fs::read_to_string(target.join("scaffold.toml")).unwrap();
        assert!(
            after.contains("# user comment"),
            "comments preserved; got:\n{after}"
        );
        assert!(
            after.contains("version = \"0.2.0\""),
            "version bumped; got:\n{after}"
        );
        assert!(
            after.contains("[repos.spel]"),
            "spel appended; got:\n{after}"
        );
        assert!(!after.contains("url ="), "url stripped; got:\n{after}");
        // Re-parse must succeed.
        parse_config(&after).expect("re-parse migrated config");
    }

    #[test]
    fn migrates_basecamp_pin_and_modules() {
        let temp = tempdir().expect("tempdir");
        let target = temp.path();
        let seed = r#"[scaffold]
version = "0.1.1"

[repos.lez]
source = "u"
pin = "abc"

[repos.spel]
source = "v"
pin = "def"

[basecamp]
pin = "deadbeef"
source = "https://github.com/logos-co/logos-basecamp"
lgpm_flake = "github:logos-co/logos-package-manager/cafef00dcafef00dcafef00dcafef00dcafef00d#cli"
port_base = 60000
port_stride = 10

[basecamp.modules.foo]
flake = "path:./foo"
role = "project"

[basecamp.modules.bar]
flake = "github:owner/bar/abc#lgx"
role = "dependency"

[wallet]
home_dir = ".scaffold/wallet"
"#;
        fs::write(target.join("scaffold.toml"), seed).expect("seed");

        cmd_init_at(target, "lgs").expect("migrate");
        let after = fs::read_to_string(target.join("scaffold.toml")).unwrap();

        // Re-parse must succeed and surface the new shape.
        let cfg = parse_config(&after).expect("re-parse migrated config");
        let bc = cfg.basecamp_repo.expect("basecamp present");
        assert_eq!(bc.pin, "deadbeef");
        assert_eq!(bc.attr, BASECAMP_ATTR);
        let lgpm = cfg.lgpm_repo.expect("lgpm present");
        assert_eq!(lgpm.pin, "cafef00dcafef00dcafef00dcafef00dcafef00d");
        assert_eq!(lgpm.attr, "cli");
        assert_eq!(cfg.modules.len(), 2);
        assert!(cfg.modules.contains_key("foo"));
        assert!(cfg.modules.contains_key("bar"));
        // [basecamp] runtime config preserved.
        let runtime = cfg.basecamp.expect("basecamp runtime present");
        assert_eq!(runtime.port_base, 60000);
        assert_eq!(runtime.port_stride, 10);
        // Old keys gone.
        assert!(
            !after.contains("lgpm_flake"),
            "lgpm_flake removed; got:\n{after}"
        );
        assert!(
            !after.contains("[basecamp.modules"),
            "basecamp.modules removed; got:\n{after}"
        );
    }

    #[test]
    fn migration_handles_unparseable_lgpm_flake() {
        let temp = tempdir().expect("tempdir");
        let target = temp.path();
        let seed = r#"[scaffold]
version = "0.1.1"

[repos.lez]
source = "u"
pin = "abc"

[repos.spel]
source = "v"
pin = "def"

[basecamp]
pin = "deadbeef"
source = "https://example.com/basecamp"
lgpm_flake = "not-a-flake-ref"

[wallet]
home_dir = ".scaffold/wallet"
"#;
        fs::write(target.join("scaffold.toml"), seed).expect("seed");
        cmd_init_at(target, "lgs").expect("migrate");

        let after = fs::read_to_string(target.join("scaffold.toml")).unwrap();
        let cfg = parse_config(&after).expect("re-parse");
        let lgpm = cfg.lgpm_repo.expect("lgpm present");
        // Default pin written in place.
        assert_eq!(lgpm.pin, DEFAULT_LGPM_PIN);
    }

    #[test]
    fn migration_strips_url_only_when_no_other_changes_needed() {
        let temp = tempdir().expect("tempdir");
        let target = temp.path();
        let seed = format!(
            r#"[scaffold]
version = "0.1.1"

[repos.lez]
url = "{}"
source = "{}"
pin = "{}"

[repos.spel]
source = "{}"
pin = "{}"

[wallet]
home_dir = ".scaffold/wallet"
"#,
            LEZ_SOURCE, LEZ_SOURCE, DEFAULT_LEZ.sha, SPEL_SOURCE, DEFAULT_SPEL.sha,
        );
        fs::write(target.join("scaffold.toml"), seed).expect("seed");
        cmd_init_at(target, "lgs").expect("migrate");
        let after = fs::read_to_string(target.join("scaffold.toml")).unwrap();
        assert!(!after.contains("url ="), "url stripped; got:\n{after}");
        parse_config(&after).expect("re-parse");
    }

    #[test]
    fn init_creates_scaffold_state_and_logs_dirs() {
        let temp = tempdir().expect("tempdir");
        let target = temp.path();
        cmd_init_at(target, "lgs").expect("init");

        assert!(target.join(".scaffold/state").is_dir());
        assert!(target.join(".scaffold/logs").is_dir());
    }

    #[test]
    fn init_gitignore_is_idempotent_with_existing_scaffold_line() {
        let temp = tempdir().expect("tempdir");
        let target = temp.path();
        fs::write(target.join(".gitignore"), "target\n.scaffold\n").expect("seed");

        cmd_init_at(target, "lgs").expect("init");

        let text = fs::read_to_string(target.join(".gitignore")).unwrap();
        let count = text.lines().filter(|l| l.trim() == ".scaffold").count();
        assert_eq!(count, 1);
    }
}
