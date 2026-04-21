use std::io::{self, Write};

use anyhow::{bail, Context};
use clap_complete::{generate, Shell};

use crate::cli::cli_command;
use crate::DynResult;

pub(crate) fn cmd_completions(shell: &str) -> DynResult<()> {
    let shell_kind = match shell {
        "bash" => Shell::Bash,
        "zsh" => Shell::Zsh,
        other => bail!(
            "unsupported shell '{other}'. Supported: bash, zsh. \
             Example: lgs completions bash > ~/.local/share/bash-completion/completions/lgs"
        ),
    };

    let mut cmd = cli_command();
    let mut buf: Vec<u8> = Vec::new();
    generate(shell_kind, &mut cmd, "lgs", &mut buf);

    let script =
        String::from_utf8(buf).context("shell completion generator produced non-UTF-8 output")?;
    let augmented = append_alias_binding(&script, shell_kind);

    io::stdout().write_all(augmented.as_bytes())?;
    Ok(())
}

fn append_alias_binding(script: &str, shell: Shell) -> String {
    let trimmed = script.trim_end_matches('\n');
    match shell {
        Shell::Bash => {
            format!("{trimmed}\ncomplete -F _lgs -o bashdefault -o default logos-scaffold\n")
        }
        Shell::Zsh => format!("{trimmed}\ncompdef _lgs logos-scaffold\n"),
        _ => script.to_string(),
    }
}
