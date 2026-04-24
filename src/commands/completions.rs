use std::io::{self, Write};

use anyhow::Context;
use clap_complete::{generate, Shell};

use crate::cli::cli_command;
use crate::DynResult;

pub(crate) fn cmd_completions(shell: Shell) -> DynResult<()> {
    let mut cmd = cli_command();
    let mut buf: Vec<u8> = Vec::new();
    generate(shell, &mut cmd, "lgs", &mut buf);

    let script =
        String::from_utf8(buf).context("shell completion generator produced non-UTF-8 output")?;
    let augmented = patch_for_alias(&script, shell);

    io::stdout().write_all(augmented.as_bytes())?;
    Ok(())
}

fn patch_for_alias(script: &str, shell: Shell) -> String {
    match shell {
        Shell::Bash => {
            let trimmed = script.trim_end_matches('\n');
            format!("{trimmed}\ncomplete -F _lgs -o bashdefault -o default logos-scaffold\n")
        }
        Shell::Zsh => {
            // Extend the `#compdef` directive so that the documented
            // `fpath` + `compinit` install recipe registers both names at
            // load time. A trailing `compdef _lgs logos-scaffold` would
            // only fire on first autoload of `_lgs`, leaving the alias
            // uncompletable until the user first tabs on `lgs`.
            if let Some(rest) = script.strip_prefix("#compdef lgs\n") {
                format!("#compdef lgs logos-scaffold\n{rest}")
            } else {
                let trimmed = script.trim_end_matches('\n');
                format!("{trimmed}\ncompdef _lgs logos-scaffold\n")
            }
        }
        _ => script.to_string(),
    }
}
