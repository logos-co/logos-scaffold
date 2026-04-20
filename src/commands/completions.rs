use std::io;

use anyhow::bail;
use clap_complete::{generate, Shell};

use crate::cli::cli_command;
use crate::DynResult;

pub(crate) fn cmd_completions(shell: &str) -> DynResult<()> {
    let shell_kind = match shell {
        "bash" => Shell::Bash,
        "zsh" => Shell::Zsh,
        other => bail!("unsupported shell '{other}'. Supported: bash, zsh."),
    };

    let mut cmd = cli_command();
    let mut stdout = io::stdout();

    for bin_name in ["logos-scaffold", "lgs"] {
        generate(shell_kind, &mut cmd, bin_name, &mut stdout);
    }

    Ok(())
}
