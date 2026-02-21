use std::env;
use std::path::PathBuf;
use std::process::Command;

use crate::commands::setup::{cmd_setup, SetupCommand, WalletInstallMode};
use crate::process::run_checked;
use crate::project::run_in_project_dir;
use crate::DynResult;

pub(crate) fn cmd_build_shortcut(project_dir: Option<PathBuf>) -> DynResult<()> {
    run_in_project_dir(project_dir.as_deref(), || {
        cmd_setup(SetupCommand {
            wallet_install: WalletInstallMode::Auto,
        })?;
        let cwd = env::current_dir()?;
        run_checked(
            Command::new("cargo")
                .current_dir(&cwd)
                .arg("build")
                .arg("--workspace"),
            "cargo build --workspace (project)",
        )?;
        Ok(())
    })
}
