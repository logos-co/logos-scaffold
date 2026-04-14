use std::env;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use crate::commands::basecamp::{build_basecamp_qml_project, parse_basecamp_build_artifact};
use crate::commands::client::generate_clients_from_current_idl;
use crate::commands::idl::build_idl_for_current_project;
use crate::commands::setup::cmd_setup;
use crate::constants::{
    FRAMEWORK_KIND_DEFAULT, FRAMEWORK_KIND_LEZ_FRAMEWORK, PROJECT_KIND_BASECAMP_QML,
    PROJECT_KIND_LEZ,
};
use crate::process::run_checked;
use crate::project::{load_project, run_in_project_dir};
use crate::DynResult;

pub(crate) fn cmd_build_shortcut(project_dir: Option<PathBuf>, artifact: String) -> DynResult<()> {
    run_in_project_dir(project_dir.as_deref(), || {
        cmd_setup()?;
        let cwd = env::current_dir()?;

        let project = load_project()?;
        match project.config.project.kind.as_str() {
            PROJECT_KIND_LEZ => match project.config.framework.kind.as_str() {
                FRAMEWORK_KIND_DEFAULT => {
                    build_workspace_for_current_project(&cwd)?;
                }
                FRAMEWORK_KIND_LEZ_FRAMEWORK => {
                    build_workspace_for_current_project(&cwd)?;
                    build_idl_for_current_project()?;
                    generate_clients_from_current_idl()?;
                }
                other => {
                    build_workspace_for_current_project(&cwd)?;
                    println!(
                        "Skipping framework-specific build steps for framework kind `{}`",
                        other
                    );
                }
            },
            PROJECT_KIND_BASECAMP_QML => {
                let artifact = parse_basecamp_build_artifact(&artifact)?;
                build_basecamp_qml_project(&project, artifact)?;
            }
            other => anyhow::bail!("unsupported project kind `{other}` for `logos-scaffold build`"),
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
