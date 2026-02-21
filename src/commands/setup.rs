use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Context;
use clap::ValueEnum;

use crate::error::SetupError;
use crate::process::{run_checked, which};
use crate::project::{ensure_dir_exists, load_project, save_project_config};
use crate::repo::sync_repo_to_pin;
use crate::state::prepare_wallet_home;
use crate::DynResult;

#[derive(Debug, Clone, Copy, ValueEnum, Default)]
pub(crate) enum WalletInstallMode {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SetupCommand {
    pub(crate) wallet_install: WalletInstallMode,
}

pub(crate) fn cmd_setup(cmd: SetupCommand) -> DynResult<()> {
    let mut project = load_project()?;
    sync_repo_to_pin(&mut project.config.lssa, "lssa")?;

    let lssa = PathBuf::from(&project.config.lssa.path);
    ensure_dir_exists(&lssa, "lssa")?;

    run_checked(
        Command::new("cargo")
            .current_dir(&lssa)
            .arg("build")
            .arg("--release")
            .arg("--features")
            .arg("standalone")
            .arg("-p")
            .arg("sequencer_runner"),
        "build sequencer_runner (standalone)",
    )?;

    ensure_wallet_install(&lssa, &project.config.wallet_binary, cmd.wallet_install)
        .context("wallet setup failed")?;

    let wallet_home = project.root.join(&project.config.wallet_home_dir);
    prepare_wallet_home(&lssa, &wallet_home)?;

    save_project_config(&project)?;
    println!("setup complete");

    Ok(())
}

fn ensure_wallet_install(
    lssa: &Path,
    wallet_binary: &str,
    mode: WalletInstallMode,
) -> DynResult<()> {
    match mode {
        WalletInstallMode::Auto => {
            if which(wallet_binary).is_some() {
                println!("wallet binary `{wallet_binary}` already present; skipping install");
                return Ok(());
            }
            run_checked(
                Command::new("cargo")
                    .current_dir(lssa)
                    .arg("install")
                    .arg("--path")
                    .arg("wallet"),
                "install wallet",
            )?;
        }
        WalletInstallMode::Always => {
            run_checked(
                Command::new("cargo")
                    .current_dir(lssa)
                    .arg("install")
                    .arg("--path")
                    .arg("wallet")
                    .arg("--force"),
                "install wallet",
            )?;
        }
        WalletInstallMode::Never => {
            if which(wallet_binary).is_none() {
                return Err(SetupError::WalletMissing {
                    binary: wallet_binary.to_string(),
                }
                .into());
            }
        }
    }

    Ok(())
}
