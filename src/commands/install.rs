use std::path::PathBuf;

use crate::commands::basecamp::install_basecamp_qml_project;
use crate::project::{ensure_basecamp_qml_project, load_project, run_in_project_dir};
use crate::DynResult;

pub(crate) fn cmd_install(project_dir: Option<PathBuf>) -> DynResult<()> {
    run_in_project_dir(project_dir.as_deref(), || {
        let project = load_project()?;
        ensure_basecamp_qml_project(&project, "logos-scaffold install")?;
        install_basecamp_qml_project(&project)
    })
}
