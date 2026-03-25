use std::env;
use std::fs;

use anyhow::{bail, Context};

use crate::config::serialize_config;
use crate::constants::{
    DEFAULT_FRAMEWORK_IDL_PATH, DEFAULT_FRAMEWORK_IDL_SPEC, DEFAULT_FRAMEWORK_VERSION,
    DEFAULT_LSSA_PIN, DEFAULT_WALLET_BINARY, FRAMEWORK_KIND_DEFAULT, LSSA_URL, VERSION,
};
use crate::model::{Config, FrameworkConfig, FrameworkIdlConfig, RepoRef};
use crate::repo::{sync_repo_to_pin_at_path_with_opts, RepoSyncOptions};
use crate::state::write_text;
use crate::DynResult;

use crate::project::default_cache_root;

pub(crate) struct InitCommand {
    pub(crate) lssa_path: Option<std::path::PathBuf>,
    pub(crate) cache_root: Option<std::path::PathBuf>,
    pub(crate) vendor_deps: bool,
}

pub(crate) fn cmd_init(cmd: InitCommand) -> DynResult<()> {
    let project_root = env::current_dir()?;

    // scaffold.toml zaten varsa dur
    let scaffold_toml = project_root.join("scaffold.toml");
    if scaffold_toml.exists() {
        bail!(
            "scaffold.toml already exists at {}. \
             This project is already initialised.\n\
             Next step: run `logos-scaffold setup` to sync dependencies.",
            scaffold_toml.display()
        );
    }

    // .scaffold dizin yapısını oluştur
    fs::create_dir_all(project_root.join(".scaffold/state"))
        .context("failed to create .scaffold/state")?;
    fs::create_dir_all(project_root.join(".scaffold/logs"))
        .context("failed to create .scaffold/logs")?;

    let cache_root = cmd.cache_root.unwrap_or(default_cache_root()?);
    fs::create_dir_all(cache_root.join("repos"))?;
    fs::create_dir_all(cache_root.join("state"))?;
    fs::create_dir_all(cache_root.join("logs"))?;
    fs::create_dir_all(cache_root.join("builds"))?;

    let lssa_source = cmd
        .lssa_path
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| LSSA_URL.to_string());

    let lssa_repo_path = if cmd.vendor_deps {
        let root = project_root.join(".scaffold/repos");
        fs::create_dir_all(&root)?;
        let lssa_vendor = root.join("lssa");
        sync_repo_to_pin_at_path_with_opts(
            &lssa_vendor,
            &lssa_source,
            DEFAULT_LSSA_PIN,
            "lssa",
            RepoSyncOptions::fail_on_source_mismatch(),
        )?;
        lssa_vendor
    } else {
        let lssa_cached = cache_root.join("repos/lssa");
        sync_repo_to_pin_at_path_with_opts(
            &lssa_cached,
            &lssa_source,
            DEFAULT_LSSA_PIN,
            "lssa",
            RepoSyncOptions::auto_reclone_cache_repo(),
        )?;
        lssa_cached
    };

    let cfg = Config {
        version: VERSION.to_string(),
        cache_root: cache_root.display().to_string(),
        lssa: RepoRef {
            url: LSSA_URL.to_string(),
            source: lssa_source,
            path: lssa_repo_path.display().to_string(),
            pin: DEFAULT_LSSA_PIN.to_string(),
        },
        wallet_binary: DEFAULT_WALLET_BINARY.to_string(),
        wallet_home_dir: ".scaffold/wallet".to_string(),
        framework: FrameworkConfig {
            kind: FRAMEWORK_KIND_DEFAULT.to_string(),
            version: DEFAULT_FRAMEWORK_VERSION.to_string(),
            idl: FrameworkIdlConfig {
                spec: DEFAULT_FRAMEWORK_IDL_SPEC.to_string(),
                path: DEFAULT_FRAMEWORK_IDL_PATH.to_string(),
            },
        },
    };

    write_text(&scaffold_toml, &serialize_config(&cfg)).context("failed to write scaffold.toml")?;

    println!("Initialised logos-scaffold in {}", project_root.display());
    println!("  scaffold.toml written");
    println!("  .scaffold/state/ created");
    println!("  .scaffold/logs/  created");
    println!("  Cache root: {}", cfg.cache_root);
    println!("  Pinned lssa: {}", cfg.lssa.pin);
    println!();
    println!("Next steps:");
    println!("  logos-scaffold setup");
    println!("  logos-scaffold localnet start");

    Ok(())
}
