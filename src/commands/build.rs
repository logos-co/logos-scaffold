use std::env;
use std::path::PathBuf;
use std::process::Command;

use crate::commands::client::build_clients_for_current_project;
use crate::commands::idl::build_idl_for_current_project;
use crate::commands::setup::cmd_setup;
use crate::constants::FRAMEWORK_KIND_LSSA_LANG;
use crate::process::run_checked;
use crate::project::{load_project, run_in_project_dir};
use crate::DynResult;

pub(crate) fn cmd_build_shortcut(args: &[String]) -> DynResult<()> {
    let mut project_dir: Option<PathBuf> = None;

    for arg in args {
        if arg.starts_with("--") {
            return Err(format!("unknown flag for build: {arg}").into());
        }

        if project_dir.is_none() {
            project_dir = Some(PathBuf::from(arg));
        } else {
            return Err(format!(
                "unexpected argument `{}`. Usage: logos-scaffold build [project-path]",
                arg
            )
            .into());
        }
    }

    run_in_project_dir(project_dir.as_deref(), || {
        cmd_setup(&[])?;
        let cwd = env::current_dir()?;
        run_checked(
            Command::new("cargo")
                .current_dir(&cwd)
                .arg("build")
                .arg("--workspace"),
            "cargo build --workspace (project)",
        )?;

        let project = load_project()?;
        if project.config.framework.kind == FRAMEWORK_KIND_LSSA_LANG {
            build_idl_for_current_project()?;
            build_clients_for_current_project()?;
        }
        Ok(())
    })
}
