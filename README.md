# logos-scaffold

`logos-scaffold` is a Rust CLI for bootstrapping local LSSA `program_deployment` projects.
It supports two templates:

- `default`: baseline hello-world runners.
- `lssa-lang`: annotation metadata, generated IDL, and generated Rust clients.

## Prerequisites

- `git`
- `rustup`, `rustc`, `cargo`
- Docker (required for guest method builds)

## Build

```bash
cargo build
# or run directly
cargo run -- --help
```

## CLI

```bash
logos-scaffold create <name> [--template default|lssa-lang] [--vendor-deps] [--lssa-path PATH] [--cache-root PATH]
logos-scaffold new <name> [--template default|lssa-lang] [--vendor-deps] [--lssa-path PATH] [--cache-root PATH]
logos-scaffold setup [--wallet-install auto|always|never]
logos-scaffold build [project-path]
logos-scaffold idl build [project-path]
logos-scaffold client build [project-path]
logos-scaffold localnet start [--timeout-sec N]
logos-scaffold localnet stop
logos-scaffold localnet status [--json]
logos-scaffold localnet logs [--tail N]
logos-scaffold doctor [--json]
logos-scaffold help
```

## Command Semantics

- `create` and `new` are aliases.
- `setup` syncs LSSA to pinned commit, builds standalone `sequencer_runner`, and installs wallet based on `--wallet-install`.
- `build` runs `setup --wallet-install auto` and then `cargo build --workspace`.
- For `lssa-lang` projects, `build` also runs IDL generation and client generation.

## Quickstart: lssa-lang

```bash
logos-scaffold new my-token-app --template lssa-lang
cd my-token-app
logos-scaffold setup
logos-scaffold localnet start
export NSSA_WALLET_HOME_DIR=$(pwd)/.scaffold/wallet
logos-scaffold build
```

## Quickstart: default

```bash
logos-scaffold new my-hello-app --template default
cd my-hello-app
logos-scaffold setup
logos-scaffold localnet start
export NSSA_WALLET_HOME_DIR=$(pwd)/.scaffold/wallet
logos-scaffold build
```

## Template Guides

- `default`: `templates/default/README.md`
- `lssa-lang`: `templates/lssa-lang/README.md`

## Troubleshooting

```bash
logos-scaffold doctor
logos-scaffold localnet status
logos-scaffold localnet logs --tail 200
```
