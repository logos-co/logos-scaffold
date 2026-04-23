use std::env;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use crate::commands::client::generate_clients_from_current_idl;
use crate::commands::idl::build_idl_for_current_project;
use crate::commands::setup::cmd_setup;
use crate::constants::{FRAMEWORK_KIND_DEFAULT, FRAMEWORK_KIND_LEZ_FRAMEWORK};
use crate::process::run_checked;
use crate::project::{load_project, run_in_project_dir};
use crate::DynResult;

pub(crate) fn cmd_build_shortcut(project_dir: Option<PathBuf>) -> DynResult<()> {
    run_in_project_dir(project_dir.as_deref(), || {
        cmd_setup()?;
        let cwd = env::current_dir()?;

        let project = load_project()?;
        match project.config.framework.kind.as_str() {
            FRAMEWORK_KIND_DEFAULT => {
                build_workspace_for_current_project(&cwd)?;
                build_methods_guests(&cwd)?;
            }
            FRAMEWORK_KIND_LEZ_FRAMEWORK => {
                build_workspace_for_current_project(&cwd)?;
                build_idl_for_current_project()?;
                generate_clients_from_current_idl()?;
                build_methods_guests(&cwd)?;
            }
            other => {
                build_workspace_for_current_project(&cwd)?;
                build_methods_guests(&cwd)?;
                println!(
                    "Skipping framework-specific build steps for framework kind `{}`",
                    other
                );
            }
        }

        Ok(())
    })
}

fn build_workspace_for_current_project(cwd: &Path) -> DynResult<()> {
    run_checked(
        Command::new("cargo")
            .current_dir(cwd)
            .arg("build")
            .arg("--workspace"),
        "cargo build --workspace (project)",
    )
}

/// Detect and build Risc0 guest binaries in the `methods/` directory.
///
/// Risc0 guest crates are intentionally excluded from the main workspace
/// because they target `riscv32im-risc0-zkvm-elf`. This function detects
/// whether a `methods/` package exists and compiles it as part of the
/// standard build pipeline.
fn build_methods_guests(cwd: &Path) -> DynResult<()> {
    let methods_manifest = cwd.join("methods").join("Cargo.toml");
    if methods_manifest.is_file() {
        println!("Building guest methods...");
        run_checked(
            Command::new("cargo")
                .current_dir(cwd)
                .arg("build")
                .arg("--release")
                .arg("--manifest-path")
                .arg(&methods_manifest),
            "cargo build --release --manifest-path methods/Cargo.toml",
        )?;
    }
    Ok(())
}
