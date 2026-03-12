# logos-scaffold CLI Completion

Completion scripts for the `logos-scaffold` command.

## ZSH

Works with both vanilla zsh and oh-my-zsh.

### Features

- Full completion for all commands and subcommands
- Contextual option completion for each command
- Descriptions for all commands and options

### Supported Commands

| Command                       | Description                                              |
|-------------------------------|----------------------------------------------------------|
| `logos-scaffold create`       | Create a new logos-scaffold project                      |
| `logos-scaffold new`          | Alias for `create`                                       |
| `logos-scaffold setup`        | Set up a logos-scaffold project                          |
| `logos-scaffold build`        | Build project (optionally `idl` or `client` subcommand)  |
| `logos-scaffold deploy`       | Deploy programs to the sequencer                         |
| `logos-scaffold localnet`     | Manage local sequencer network (start, stop, status, logs) |
| `logos-scaffold wallet`       | Manage wallet and accounts (list, topup, default)        |
| `logos-scaffold doctor`       | Diagnose project environment                             |
| `logos-scaffold report`       | Collect sanitized diagnostics archive                    |

### Installation

#### Vanilla Zsh

1. Create a completions directory:

   ```sh
   mkdir -p ~/.zsh/completions
   ```

2. Copy the completion file:

   ```sh
   cp ./completions/_logos-scaffold ~/.zsh/completions/
   ```

3. Add to your `~/.zshrc` (before any `compinit` call, or add these lines if you don't have one):

   ```sh
   fpath=(~/.zsh/completions $fpath)
   autoload -Uz compinit && compinit
   ```

4. Reload your shell:

   ```sh
   exec zsh
   ```

#### Oh-My-Zsh

1. Create the plugin directory and copy the file:

   ```sh
   mkdir -p ~/.oh-my-zsh/custom/plugins/logos-scaffold
   cp completions/_logos-scaffold ~/.oh-my-zsh/custom/plugins/logos-scaffold/
   ```

2. Add `logos-scaffold` to your plugins array in `~/.zshrc`:

   ```sh
   plugins=(... logos-scaffold)
   ```

3. Reload your shell:

   ```sh
   exec zsh
   ```

### Usage

```sh
# Top-level commands
logos-scaffold <TAB>

# localnet subcommands
logos-scaffold localnet <TAB>

# wallet subcommands
logos-scaffold wallet <TAB>

# Options for wallet topup
logos-scaffold wallet topup --<TAB>

# Template choices when creating a project
logos-scaffold create my-project --template <TAB>
# Shows: default  lez-framework

# Wallet install mode choices
logos-scaffold setup --wallet-install <TAB>
# Shows: auto  always  never

# Pass arbitrary arguments directly to the wallet binary
logos-scaffold wallet -- account list
```

## Troubleshooting

### Completions not appearing

1. Check that `compinit` is called in your `.zshrc`
2. Rebuild the completion cache:

   ```sh
   rm -f ~/.zcompdump*
   exec zsh
   ```
