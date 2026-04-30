use std::env;
use std::fs;
use std::path::Path;

use anyhow::{bail, Context};

use crate::config::serialize_config;
use crate::constants::{
    DEFAULT_FRAMEWORK_IDL_PATH, DEFAULT_FRAMEWORK_IDL_SPEC, DEFAULT_FRAMEWORK_VERSION,
    DEFAULT_LEZ_PIN, FRAMEWORK_KIND_DEFAULT, LEZ_URL, VERSION,
};
use crate::model::{
    Config, FrameworkConfig, FrameworkIdlConfig, LocalnetConfig, RepoRef, RunConfig,
};
use crate::project::default_cache_root;
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
        bail!(
            "scaffold.toml already exists in {}; refusing to overwrite",
            target.display()
        );
    }

    let (bootstrap_cache, _) = default_cache_root()?;
    let lez_path = bootstrap_cache
        .join("repos/lez")
        .join(DEFAULT_LEZ_PIN)
        .display()
        .to_string();

    let cfg = Config {
        version: VERSION.to_string(),
        cache_root: String::new(),
        lez: RepoRef {
            url: LEZ_URL.to_string(),
            source: LEZ_URL.to_string(),
            path: lez_path,
            pin: DEFAULT_LEZ_PIN.to_string(),
        },
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
        run: RunConfig::default(),
        basecamp: None,
    };

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse_config;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn init_writes_parseable_scaffold_toml() {
        let temp = tempdir().expect("tempdir");
        let target = temp.path();
        cmd_init_at(target, "lgs").expect("init");

        let text = fs::read_to_string(target.join("scaffold.toml")).expect("read scaffold.toml");
        let cfg = parse_config(&text).expect("parse scaffold.toml");

        assert_eq!(cfg.version, VERSION);
        assert_eq!(cfg.lez.pin, DEFAULT_LEZ_PIN);
        assert_eq!(cfg.framework.kind, FRAMEWORK_KIND_DEFAULT);
        assert_eq!(cfg.wallet_home_dir, ".scaffold/wallet");
        assert_eq!(cfg.localnet.port, 3040);
        assert!(cfg.localnet.risc0_dev_mode);
    }

    #[test]
    fn init_does_not_persist_cache_root() {
        let temp = tempdir().expect("tempdir");
        let target = temp.path();
        cmd_init_at(target, "lgs").expect("init");

        let text = fs::read_to_string(target.join("scaffold.toml")).expect("read scaffold.toml");
        let has_active_cache_root = text
            .lines()
            .any(|l| !l.trim_start().starts_with('#') && l.contains("cache_root"));
        assert!(
            !has_active_cache_root,
            "scaffold.toml should not pin cache_root by default; got:\n{text}"
        );

        let cfg = parse_config(&text).expect("parse scaffold.toml");
        assert!(
            cfg.cache_root.is_empty(),
            "parsed cache_root should be empty; got {:?}",
            cfg.cache_root
        );
    }

    #[test]
    fn init_refuses_when_scaffold_toml_exists() {
        let temp = tempdir().expect("tempdir");
        let target = temp.path();
        fs::write(target.join("scaffold.toml"), "# existing\n").expect("seed");

        let err = cmd_init_at(target, "lgs").expect_err("should refuse");
        assert!(
            err.to_string().contains("already exists"),
            "unexpected error: {err}"
        );

        let after = fs::read_to_string(target.join("scaffold.toml")).unwrap();
        assert_eq!(after, "# existing\n");
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
