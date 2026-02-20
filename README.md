# logos-scaffold

`logos-scaffold` is a Rust CLI that bootstraps local LSSA development projects.
It supports two templates:

- `default`: hello-world style examples from `program_deployment`.
- `lssa-lang`: Anchor-inspired DX with annotation metadata, IDL generation, and generated Rust clients.

## Prerequisites

- `git`
- `rustup`, `rustc`, `cargo`
- Docker (required for guest method builds)

## Install / Build

```bash
cargo build
# or run directly from source
cargo run -- --help
```

## CLI

```bash
logos-scaffold create <name> [--template default|lssa-lang] [--vendor-deps] [--lssa-path PATH] [--cache-root PATH]
logos-scaffold new <name> [--template default|lssa-lang] [--vendor-deps] [--lssa-path PATH] [--cache-root PATH]
logos-scaffold build [project-path]
logos-scaffold idl build [project-path]
logos-scaffold client build [project-path]
logos-scaffold setup
logos-scaffold localnet start
logos-scaffold localnet stop
logos-scaffold localnet status
logos-scaffold localnet logs [--tail N]
logos-scaffold doctor
```

## Quickstart: `lssa-lang` (Token Pilot)

```bash
logos-scaffold new my-token-app --template lssa-lang
cd my-token-app

logos-scaffold setup
logos-scaffold localnet start
export NSSA_WALLET_HOME_DIR=$(pwd)/.scaffold/wallet

# Build workspace, then generate IDL + client
logos-scaffold build

# Create accounts (copy account IDs from wallet output)
wallet account new public
wallet account new public
wallet account new private

# Example flow
cargo run --bin run_token_pilot -- init --to <public_account_id>
cargo run --bin run_token_pilot -- transfer --from <public_sender_id> --to <public_recipient_id> --amount 10
cargo run --bin run_token_pilot -- transfer --from <public_sender_id> --to Private/<private_recipient_id> --amount 7
wallet account sync-private
```

## Quickstart: `default` (Hello World)

```bash
logos-scaffold new my-hello-app --template default
cd my-hello-app

logos-scaffold setup
logos-scaffold localnet start
export NSSA_WALLET_HOME_DIR=$(pwd)/.scaffold/wallet
logos-scaffold build

wallet account new public
cargo run --bin run_hello_world -- <public_account_id>
```

## Which Template Should I Use?

- Choose `lssa-lang` if you want typed instruction/account metadata, generated IDL, and generated Rust clients.
- Choose `default` if you want the raw hello-world style examples and manual runner flow.

## Template Guides

- `default` guide: `templates/default/README.md`
- `lssa-lang` guide: `templates/lssa-lang/README.md`
- `lssa-lang` macro guide section: `templates/lssa-lang/README.md#macro-guide`

## Troubleshooting

```bash
logos-scaffold doctor
logos-scaffold localnet status
logos-scaffold localnet logs --tail 200
```

If localnet is down, start it:

```bash
logos-scaffold localnet start
```
