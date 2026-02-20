use std::path::PathBuf;
use std::process::Command;

use crate::process::run_checked;
use crate::project::{ensure_dir_exists, load_project, save_project_config};
use crate::repo::sync_repo_to_pin;
use crate::state::prepare_wallet_home;
use crate::DynResult;

pub(crate) fn cmd_setup(args: &[String]) -> DynResult<()> {
    if !args.is_empty() {
        return Err("usage: logos-scaffold setup".into());
    }

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

    run_checked(
        Command::new("cargo")
            .current_dir(&lssa)
            .arg("install")
            .arg("--path")
            .arg("wallet")
            .arg("--force"),
        "install wallet",
    )?;

    let wallet_home = project.root.join(&project.config.wallet_home_dir);
    prepare_wallet_home(&lssa, &wallet_home)?;

    save_project_config(&project)?;
    println!("setup complete");

    Ok(())
}
