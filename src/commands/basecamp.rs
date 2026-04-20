use std::path::PathBuf;

use anyhow::{bail, Context};

use crate::project::load_project;
use crate::DynResult;

#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields wired up in later phases
pub(crate) enum BasecampAction {
    Setup,
    Install {
        path: Option<PathBuf>,
        flake: Option<String>,
        profile: Option<String>,
    },
    Launch {
        profile: String,
        no_clean: bool,
    },
    ProfileList {
        json: bool,
    },
}

pub(crate) fn cmd_basecamp(action: BasecampAction) -> DynResult<()> {
    let _project = load_project().context(
        "This command must be run inside a logos-scaffold project.\nNext step: cd into your scaffolded project directory and retry.",
    )?;

    match action {
        BasecampAction::Setup => bail!("basecamp setup is not yet implemented"),
        BasecampAction::Install { .. } => bail!("basecamp install is not yet implemented"),
        BasecampAction::Launch { .. } => bail!("basecamp launch is not yet implemented"),
        BasecampAction::ProfileList { .. } => bail!("basecamp profile list is not yet implemented"),
    }
}
